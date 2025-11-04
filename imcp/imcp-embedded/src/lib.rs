#![no_std]

use core::{error::Error, fmt::Display};

use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Timer};
use embedded_hal::digital::OutputPin;
use embedded_io_async::{ErrorType, Read, Write};

pub struct ImcpEmbedded<U, D>
where
    U: Write + Read,
    D: OutputPin,
{
    uart: U,
    de_pin: Option<D>,
    byte_time_micro: u64,
    tx_finish_margin_micro: u64,
}

impl<U, D> ImcpEmbedded<U, D>
where
    U: Write + Read,
    D: OutputPin,
{
    pub fn new(uart: U, mut de_pin: Option<D>, baud_rate: u32) -> Result<Self, D::Error> {
        let byte_time_us: u64 = ((1_000_000 * 10) / baud_rate).into();
        // マージンも設定 (例: 1バイト分)
        let tx_finish_margin_us = byte_time_us;

        // 初期状態を受信モードに設定
        if let Some(ref mut pin) = de_pin {
            pin.set_low()?
        }

        Ok(Self {
            uart,
            de_pin,
            byte_time_micro: byte_time_us,
            tx_finish_margin_micro: tx_finish_margin_us,
        })
    }
}

impl<U, D> Read for ImcpEmbedded<U, D>
where
    U: Write + Read,
    D: OutputPin,
{
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.uart.read(buf).await.map_err(ImcpEmbeddedError::Uart)
    }
}

impl<U, D> Write for ImcpEmbedded<U, D>
where
    U: Write + Read,
    D: OutputPin,
{
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }

        // 1. キャリアセンス (バスが空くまで待機)
        // (アイドル時間は 1.5 バイト分など、プロトコルで定義)
        let idle_duration =
            Duration::from_micros(self.byte_time_micro + (self.byte_time_micro / 2));
        let mut read_buf = [0u8; 1];

        loop {
            match select(self.uart.read(&mut read_buf), Timer::after(idle_duration)).await {
                Either::Second(_) => {
                    // タイマーが完了 = バスはアイドル
                    break;
                }
                Either::First(Ok(_)) => {
                    // データを受信 = バスはビジー、監視を継続
                    continue;
                }
                Either::First(Err(e)) => {
                    // UART読み取りエラー
                    // (エラーを無視して継続するか、即時エラーとするか)
                    return Err(ImcpEmbeddedError::Uart(e));
                }
            }
        }

        // 2. 送信モードに設定 (DE=HIGH)
        // (OutputPin::Error を Rs485Error::Pin にマッピング)
        if let Some(pin) = self.de_pin.as_mut() {
            pin.set_high().map_err(ImcpEmbeddedError::Pin)?;
        }

        // 3. 内部の UART を使ってデータを送信
        let written_len = self
            .uart
            .write(buf)
            .await
            .map_err(ImcpEmbeddedError::Uart)?;

        // 4. 送信完了待機 (Timer)
        let wait_us = self.byte_time_micro * written_len as u64;
        Timer::after(Duration::from_micros(wait_us + self.tx_finish_margin_micro)).await;

        // 5. 受信モードに戻す (DE=LOW)
        if let Some(pin) = self.de_pin.as_mut() {
            pin.set_low().map_err(ImcpEmbeddedError::Pin)?;
        }

        Ok(written_len)
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.uart.flush().await.map_err(ImcpEmbeddedError::Uart)
    }
}

/// UARTエラーとPinエラーを統合するカスタムエラー型
#[derive(Debug)]
pub enum ImcpEmbeddedError<UE, PE> {
    /// 内部のUART (Read/Write) からのエラー
    Uart(UE),
    /// DE/REピン (OutputPin) からのエラー
    Pin(PE),
}

impl<UE, PE> Error for ImcpEmbeddedError<UE, PE>
where
    UE: core::fmt::Debug, // エラーはデバッグ表示できる必要がある
    PE: core::fmt::Debug,
{
}

impl<UE, PE> Display for ImcpEmbeddedError<UE, PE>
where
    UE: core::fmt::Debug, // エラーはデバッグ表示できる必要がある
    PE: core::fmt::Debug,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ImcpEmbeddedError::Uart(ue) => write!(f, "UART Error {:?}", ue),
            ImcpEmbeddedError::Pin(pe) => write!(f, "UART Error {:?}", pe),
        }
    }
}

/// embedded-io-async の Error トレイトを実装
impl<UE, PE> embedded_io_async::Error for ImcpEmbeddedError<UE, PE>
where
    UE: core::fmt::Debug, // エラーはデバッグ表示できる必要がある
    PE: core::fmt::Debug,
{
    fn kind(&self) -> embedded_io_async::ErrorKind {
        // 必要に応じて、UE の kind() を返す実装も可能
        embedded_io_async::ErrorKind::Other
    }
}

/// ErrorType トレイトの実装
impl<U, P> ErrorType for ImcpEmbedded<U, P>
where
    U: Read + Write,
    P: OutputPin,
{
    type Error = ImcpEmbeddedError<U::Error, P::Error>;
}

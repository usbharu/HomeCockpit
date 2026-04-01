#![no_std]

use core::{error::Error, fmt::Display};

use embassy_time::{Duration, Timer};
use embedded_hal::digital::OutputPin;
use embedded_io_async::{ErrorType, Read, Write};

#[cfg(feature = "embassy-rp")]
use embassy_rp::{
    pac::uart::Uart as RpUartRegs,
    uart::BufferedUart,
};
#[cfg(feature = "embassy-rp")]
use embedded_io_async::{BufRead, ReadReady};

#[allow(async_fn_in_trait)]
pub trait CarrierSenseUart: Read + Write {
    async fn wait_bus_idle(
        &mut self,
        idle_for_us: u64,
        sample_interval_us: u64,
    ) -> Result<(), Self::Error>;
}

pub struct ImcpEmbedded<U, D>
where
    U: CarrierSenseUart,
    D: OutputPin,
{
    uart: U,
    de_pin: Option<D>,
    byte_time_micro: u64,
    tx_finish_margin_micro: u64,
}

impl<U, D> ImcpEmbedded<U, D>
where
    U: CarrierSenseUart,
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
    U: CarrierSenseUart,
    D: OutputPin,
{
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        Read::read(&mut self.uart, buf)
            .await
            .map_err(ImcpEmbeddedError::Uart)
    }
}

impl<U, D> Write for ImcpEmbedded<U, D>
where
    U: CarrierSenseUart,
    D: OutputPin,
{
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }

        self.uart
            .wait_bus_idle(
                self.byte_time_micro + (self.byte_time_micro / 2),
                (self.byte_time_micro / 8).max(1),
            )
            .await
            .map_err(ImcpEmbeddedError::Uart)?;

        // 2. 送信モードに設定 (DE=HIGH)
        // (OutputPin::Error を Rs485Error::Pin にマッピング)
        if let Some(pin) = self.de_pin.as_mut() {
            pin.set_high().map_err(ImcpEmbeddedError::Pin)?;
        }

        // 3. 内部の UART を使ってデータを送信
        let write_result = Write::write_all(&mut self.uart, buf).await;

        let buf_len = buf.len();

        // 4. 送信完了待機 (Timer)
        let wait_us = self.byte_time_micro * buf_len as u64;
        Timer::after(Duration::from_micros(wait_us + self.tx_finish_margin_micro)).await;

        // 5. 受信モードに戻す (DE=LOW)
        if let Some(pin) = self.de_pin.as_mut() {
            pin.set_low().map_err(ImcpEmbeddedError::Pin)?;
        }

        write_result.map_err(ImcpEmbeddedError::Uart)?;

        Ok(buf_len)
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        Write::flush(&mut self.uart)
            .await
            .map_err(ImcpEmbeddedError::Uart)
    }
}

/// UARTエラーとPinエラーを統合するカスタムエラー型
#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ImcpEmbeddedError<UE, PE>
where
    UE: core::fmt::Debug, // エラーはデバッグ表示できる必要がある
    PE: core::fmt::Debug,
{
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
    U: CarrierSenseUart,
    P: OutputPin,
{
    type Error = ImcpEmbeddedError<U::Error, P::Error>;
}

#[cfg(feature = "embassy-rp")]
pub struct RpUartCarrierSense {
    uart: BufferedUart,
    regs: RpUartRegs,
}

#[cfg(feature = "embassy-rp")]
impl RpUartCarrierSense {
    pub fn new(uart: BufferedUart, regs: RpUartRegs) -> Self {
        Self { uart, regs }
    }
}

#[cfg(feature = "embassy-rp")]
impl ErrorType for RpUartCarrierSense {
    type Error = <BufferedUart as ErrorType>::Error;
}

#[cfg(feature = "embassy-rp")]
impl Read for RpUartCarrierSense {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        Read::read(&mut self.uart, buf).await
    }
}

#[cfg(feature = "embassy-rp")]
impl Write for RpUartCarrierSense {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        Write::write(&mut self.uart, buf).await
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        Write::flush(&mut self.uart).await
    }
}

#[cfg(feature = "embassy-rp")]
impl CarrierSenseUart for RpUartCarrierSense {
    async fn wait_bus_idle(
        &mut self,
        idle_for_us: u64,
        sample_interval_us: u64,
    ) -> Result<(), Self::Error> {
        let sample_us = sample_interval_us.max(1);
        let mut quiet_us = 0;

        while quiet_us < idle_for_us {
            let fr = self.regs.uartfr().read();
            let rx_buffered = if ReadReady::read_ready(&mut self.uart)? {
                let buffered: &[u8] = BufRead::fill_buf(&mut self.uart).await?;
                !buffered.is_empty()
            } else {
                false
            };

            if fr.busy() || !fr.rxfe() || rx_buffered {
                quiet_us = 0;
            } else {
                quiet_us = quiet_us.saturating_add(sample_us);
            }

            Timer::after(Duration::from_micros(sample_us)).await;
        }

        Ok(())
    }
}

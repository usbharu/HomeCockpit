"use client";

import * as Tooltip from "@radix-ui/react-tooltip";
import { CircleHelp } from "lucide-react";
import type { ReactNode } from "react";

type HelpTipProps = {
  content: string;
  side?: "top" | "right" | "bottom" | "left";
  children?: ReactNode;
};

const contentClassName =
  "z-50 max-w-xs rounded-md border border-gray-700 bg-gray-900 px-3 py-2 text-xs leading-relaxed text-white shadow-lg";

export function HelpTip({ content, side = "top", children }: HelpTipProps) {
  return (
    <Tooltip.Root>
      <Tooltip.Trigger asChild>
        {children ?? (
          <button
            type="button"
            className="inline-flex shrink-0 items-center justify-center rounded-full text-gray-400 transition hover:text-gray-600 focus:outline-none focus-visible:ring-2 focus-visible:ring-blue-500 focus-visible:ring-offset-1"
            aria-label="説明"
            onPointerDown={(event) => event.stopPropagation()}
            onClick={(event) => event.stopPropagation()}
          >
            <CircleHelp size={15} aria-hidden />
          </button>
        )}
      </Tooltip.Trigger>
      <Tooltip.Portal>
        <Tooltip.Content side={side} sideOffset={6} className={contentClassName}>
          {content}
          <Tooltip.Arrow className="fill-gray-900" />
        </Tooltip.Content>
      </Tooltip.Portal>
    </Tooltip.Root>
  );
}

type LabelWithHelpProps = {
  label: ReactNode;
  tip: string;
  className?: string;
};

export function LabelWithHelp({ label, tip, className }: LabelWithHelpProps) {
  return (
    <span className={`inline-flex items-center gap-1.5 ${className ?? ""}`}>
      {label}
      <HelpTip content={tip} />
    </span>
  );
}

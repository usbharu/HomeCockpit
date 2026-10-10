"use client";

import * as Tooltip from "@radix-ui/react-tooltip";
import { CircleHelp } from "lucide-react";
import { useId, useState, type ReactNode } from "react";

import {
  managerTooltipAriaLabels,
  managerTooltips,
  type ManagerTooltipKey,
} from "@/lib/manager-tooltips";

type HelpTipProps = {
  content: string;
  ariaLabel: string;
  side?: "top" | "right" | "bottom" | "left";
  children?: ReactNode;
};

const contentClassName =
  "z-50 max-w-xs rounded-md border border-gray-700 bg-gray-900 px-3 py-2 text-xs leading-relaxed text-white shadow-lg";

export function HelpTip({ content, ariaLabel, side = "top", children }: HelpTipProps) {
  const [open, setOpen] = useState(false);
  const contentId = useId();

  return (
    <Tooltip.Root open={open} onOpenChange={setOpen}>
      <Tooltip.Trigger asChild>
        {children ?? (
          <button
            type="button"
            className="inline-flex shrink-0 items-center justify-center rounded-full text-gray-400 transition hover:text-gray-600 focus:outline-none focus-visible:ring-2 focus-visible:ring-blue-500 focus-visible:ring-offset-1"
            aria-label={ariaLabel}
            aria-expanded={open}
            aria-controls={contentId}
            onPointerDown={(event) => event.stopPropagation()}
            onClick={(event) => {
              event.stopPropagation();
              setOpen((current) => !current);
            }}
          >
            <CircleHelp size={15} aria-hidden />
          </button>
        )}
      </Tooltip.Trigger>
      <Tooltip.Portal>
        <Tooltip.Content id={contentId} side={side} sideOffset={6} className={contentClassName}>
          {content}
          <Tooltip.Arrow className="fill-gray-900" />
        </Tooltip.Content>
      </Tooltip.Portal>
    </Tooltip.Root>
  );
}

type ManagerHelpTipProps = {
  tipKey: ManagerTooltipKey;
  side?: HelpTipProps["side"];
};

export function ManagerHelpTip({ tipKey, side }: ManagerHelpTipProps) {
  return (
    <HelpTip
      content={managerTooltips[tipKey]}
      ariaLabel={managerTooltipAriaLabels[tipKey]}
      side={side}
    />
  );
}

type LabelWithHelpProps = {
  label: ReactNode;
  tipKey: ManagerTooltipKey;
  className?: string;
};

export function LabelWithHelp({ label, tipKey, className }: LabelWithHelpProps) {
  return (
    <span className={`inline-flex items-center gap-1.5 ${className ?? ""}`}>
      {label}
      <ManagerHelpTip tipKey={tipKey} />
    </span>
  );
}

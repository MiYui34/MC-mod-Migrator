import * as React from "react";
import { cn } from "@/lib/utils";

const Badge = React.forwardRef<
  HTMLDivElement,
  React.HTMLAttributes<HTMLDivElement> & {
    variant?: "default" | "success" | "warning" | "destructive" | "secondary";
  }
>(({ className, variant = "default", ...props }, ref) => {
  const variants = {
    default: "bg-[var(--color-primary)]/20 text-[var(--color-primary)]",
    success: "bg-emerald-500/20 text-emerald-400",
    warning: "bg-amber-500/20 text-amber-400",
    destructive: "bg-red-500/20 text-red-400",
    secondary: "bg-[var(--color-secondary)] text-[var(--color-muted-foreground)]",
  };
  return (
    <div
      ref={ref}
      className={cn(
        "inline-flex items-center rounded-full px-2.5 py-0.5 text-xs font-medium whitespace-nowrap shrink-0",
        variants[variant],
        className
      )}
      {...props}
    />
  );
});
Badge.displayName = "Badge";

export { Badge };

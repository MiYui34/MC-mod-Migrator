import { cn } from "@/lib/utils";

interface ProgressProps {
  value: number;
  className?: string;
}

export function Progress({ value, className }: ProgressProps) {
  const clamped = Math.min(100, Math.max(0, value));
  return (
    <div
      className={cn(
        "relative h-2 w-full overflow-hidden rounded-full bg-[var(--color-secondary)]",
        className
      )}
    >
      <div
        className="h-full bg-[var(--color-primary)] transition-all duration-300"
        style={{ width: `${clamped}%` }}
      />
    </div>
  );
}

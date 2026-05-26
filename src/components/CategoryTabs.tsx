import type { MigrationCategory } from "@/types";
import { CATEGORY_LABELS } from "@/types";
import { cn } from "@/lib/utils";

const CATEGORIES: MigrationCategory[] = [
  "mod",
  "shader_pack",
  "resource_pack",
  "datapack",
  "litematica",
  "mod_config",
  "game_settings",
];

interface CategoryTabsProps {
  active: MigrationCategory;
  onChange: (category: MigrationCategory) => void;
}

export function CategoryTabs({ active, onChange }: CategoryTabsProps) {
  return (
    <div className="flex flex-nowrap gap-1 px-4 py-2 border-b border-[var(--color-border)] bg-[var(--color-muted)]/20 overflow-x-auto">
      {CATEGORIES.map((cat) => (
        <button
          key={cat}
          type="button"
          onClick={() => onChange(cat)}
          className={cn(
            "px-3 py-1.5 text-xs rounded-md transition-colors",
            active === cat
              ? "bg-[var(--color-primary)] text-[var(--color-primary-foreground)]"
              : "hover:bg-[var(--color-muted)] text-[var(--color-muted-foreground)]"
          )}
        >
          {CATEGORY_LABELS[cat]}
        </button>
      ))}
    </div>
  );
}

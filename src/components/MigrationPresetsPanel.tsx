import { BookmarkPlus, Trash2 } from "lucide-react";
import { useState } from "react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { InstanceInfo, MigrationPreset } from "@/types";

interface MigrationPresetsPanelProps {
  presets: MigrationPreset[];
  sourceInstance: InstanceInfo | null;
  targetInstance: InstanceInfo | null;
  onSave: (preset: MigrationPreset) => Promise<unknown>;
  onDelete: (id: string) => Promise<void>;
  onApply: (preset: MigrationPreset) => void;
}

export function MigrationPresetsPanel({
  presets,
  sourceInstance,
  targetInstance,
  onSave,
  onDelete,
  onApply,
}: MigrationPresetsPanelProps) {
  const [name, setName] = useState("");
  const [saving, setSaving] = useState(false);

  const handleSave = async () => {
    if (!name.trim()) return;
    setSaving(true);
    try {
      await onSave({
        id: "",
        name: name.trim(),
        sourceMc: sourceInstance?.mcVersion ?? "",
        sourceLoader: sourceInstance?.loader ?? "",
        targetMc: targetInstance?.mcVersion ?? "",
        targetLoader: targetInstance?.loader ?? "",
        backupBeforeTransfer: true,
        modReportFormat: "md",
        modVersionPolicy: "auto",
      });
      setName("");
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="space-y-2 pt-2 border-t border-[var(--color-border)]">
      <p className="text-xs font-medium text-[var(--color-muted-foreground)]">迁移预设</p>
      <div className="flex gap-1">
        <Input
          className="h-8 text-xs"
          placeholder="预设名称"
          value={name}
          onChange={(e) => setName(e.target.value)}
        />
        <Button
          size="sm"
          variant="outline"
          className="shrink-0 h-8"
          disabled={saving || !name.trim()}
          onClick={() => void handleSave()}
        >
          <BookmarkPlus className="h-3.5 w-3.5" />
        </Button>
      </div>
      {presets.length > 0 && (
        <ul className="space-y-1 max-h-28 overflow-y-auto">
          {presets.map((p) => (
            <li
              key={p.id}
              className="flex items-center justify-between gap-1 rounded border border-[var(--color-border)] px-2 py-1 text-xs"
            >
              <button
                type="button"
                className="text-left flex-1 truncate hover:underline"
                onClick={() => onApply(p)}
                title={`${p.sourceLoader || "?"} → ${p.targetLoader || "?"}`}
              >
                {p.name}
              </button>
              <button
                type="button"
                className="text-[var(--color-muted-foreground)] hover:text-red-400"
                onClick={() => void onDelete(p.id)}
              >
                <Trash2 className="h-3 w-3" />
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

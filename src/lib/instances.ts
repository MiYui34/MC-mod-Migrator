import type { InstanceInfo } from "@/types";

export const SAME_GAME_VERSION_FOLDER_ERROR =
  "源实例与目标实例不能为同一游戏版本文件夹";

export function instanceGameDirKey(instance: InstanceInfo): string {
  const dir =
    instance.gameDir?.trim() ||
    instance.modsPath.replace(/[/\\]mods\/?$/i, "");
  return dir.replace(/\\/g, "/").toLowerCase();
}

export function instancesSameGameFolder(
  a: InstanceInfo,
  b: InstanceInfo
): boolean {
  return instanceGameDirKey(a) === instanceGameDirKey(b);
}

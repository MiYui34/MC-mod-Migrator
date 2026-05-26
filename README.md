# MC Mod 换端助手

跨 Minecraft 版本/端迁移 Mod 与游戏资源的桌面工具。支持从 Modrinth、CurseForge（含 MCIM 镜像）识别、下载并安装到目标实例。

## 功能

### 迁移（7 类）

| 分类 | 说明 |
|------|------|
| Mod | 扫描识别 `.jar`，检查兼容性，自动下载目标端适配版本 |
| 光影包 | 扫描 `shaderpacks/`，支持在线检测与迁移 |
| 材质包 | 扫描 `resourcepacks/` |
| 数据包 | 扫描 `datapacks/` |
| 投影文件 | Litematica 等 schematic |
| Mod 配置 | 相关或全部 `config/` |
| 游戏设置 | `options.txt` 等 |

- 多源识别：Modrinth（SHA512）、CurseForge（fingerprint）、jar 元数据、MC百科、GitHub
- 迁移前备份与撤销
- 自动检测 `.minecraft/versions/` 实例的 MC 版本与加载器
- 发现 HMCL / PCL2 / 手动 `.minecraft` 实例

### 市场（5 类）

| 分类 | 安装位置 |
|------|----------|
| 光影包 | `shaderpacks/` |
| 资源包 | `resourcepacks/` |
| Mod | `mods/`（可选自动拉取依赖） |
| 整合包 | Modrinth `.mrpack` 完整安装；CF 整合包下载后提示在 PCL/HMCL 导入 |
| 数据包 | `datapacks/` |

- 来源筛选（Modrinth / CurseForge / 全部）、排序、MC 版本与加载器覆盖
- 安装队列与进度条、批量安装
- 最近安装记录与撤销（轻量备份）
- 迁移页可跳转市场搜索或一键从市场安装（光影/材质在线可用项）

## 技术栈

- Tauri 2 + Rust
- React + TypeScript + Tailwind CSS

## 前置要求

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://www.rust-lang.org/tools/install)（Tauri 构建需要）
- Windows 10/11 上还需安装 [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)（勾选「使用 C++ 的桌面开发」）以提供 `link.exe`

## 开发

```bash
cd mc-mod-migrator
npm install
npm run tauri dev
```

## 构建

```bash
npm run tauri build
```

## 设置

- **CurseForge API Key**（可选）：在「设置」页填入 [CurseForge API Key](https://console.curseforge.com/) 以启用 CurseForge 搜索与下载。Modrinth 无需 Key。
- **MCIM 镜像**：设置中的 `mod_api_mirror` 可指向 MCIM 等镜像，下载时会自动回退官方 CDN。

## 使用流程

### 迁移

1. 选择**源实例** → 选择分类 → 点击「扫描」
2. 选择**目标实例** → Mod 分类需「检查兼容性」
3. 勾选项目 → 「开始迁移」

### 市场

1. 切换到顶部 **市场** 模式
2. 选择目标实例（左侧）
3. 选择分类、搜索、筛选来源/排序
4. 选择版本 → 「立即安装」或「加入队列」
5. Mod 安装可勾选「同时安装必需依赖」

### 整合包说明

- **Modrinth `.mrpack`**：解析 `modrinth.index.json`，按索引下载 mods、资源等到实例目录
- **CurseForge 等非 mrpack**：保存到 `{game_dir}/downloads/`，请在 PCL 或 HMCL 中手动导入

## 测试

```bash
cd src-tauri && cargo test
npm run build
```

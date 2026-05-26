# MC 换端助手

面向 **PCL2 / HMCL** 等启动器的 Minecraft **整合包换端**桌面工具：在多个游戏实例之间迁移 Mod 与游戏资源，并通过多源市场浏览、安装与更新。

当前版本：**0.2.1** · 技术栈：**Tauri 2** + **Rust** · **React** + **TypeScript** + **Tailwind CSS** · 许可：[MIT](./LICENSE)

---

## 功能概览

### 迁移（7 类资源）

| 分类 | 说明 |
|------|------|
| Mod | 扫描识别 `.jar`，兼容性检查，自动下载目标端适配版本 |
| 光影包 | 扫描 `shaderpacks/`，支持在线检测与迁移 |
| 材质包 | 扫描 `resourcepacks/` |
| 数据包 | 扫描 `datapacks/` |
| 投影文件 | Litematica 等 schematic |
| Mod 配置 | 相关或全部 `config/` |
| 游戏设置 | `options.txt` 等 |

**迁移能力**

- **多源识别**：Modrinth（SHA512）、CurseForge（fingerprint）、jar 元数据、MC百科、GitHub
- **迁移检查**：重复 jar、Mod ID 冲突、加载器混装、跨大版本换端清单等提醒
- **Mod 对比**：源/目标实例 Mod 差异（仅源有、仅目标有、版本不一致、已对齐）
- **迁移预设**：保存常用源/目标 MC 版本、加载器与备份策略，一键套用
- **备份与撤销**：迁移前备份被覆盖文件；支持按记录撤销（需开启「迁移前备份」）
- **迁移报告**：可选在 Mod 迁移完成后导出 Markdown / 纯文本报告
- **实例发现**：自动扫描 HMCL、PCL2、`.minecraft/versions/` 及手动选择的实例目录

### 市场（6 类）

| 分类 | 安装位置 |
|------|----------|
| 光影包 | `shaderpacks/` |
| 资源包 | `resourcepacks/` |
| Mod | `mods/`（可选自动拉取依赖） |
| 整合包 | Modrinth `.mrpack` 完整安装；CF 等保存后提示在 PCL/HMCL 导入 |
| 数据包 | `datapacks/` |
| 投影文件 | 从 SGU 投影站下载 Litematica |

**市场能力**

- 来源筛选（Modrinth / CurseForge / 全部）、排序、MC 版本与加载器覆盖
- **中文搜索**：MC百科桥接 + PCL 风格结果合并排序
- **发现 / 搜索 / Mod 更新 / 缺失依赖** 四个视图；安装队列、批量安装与进度
- **最近安装与撤销**（轻量备份）
- 迁移页可跳转市场，或对光影/材质等「在线可用」项一键安装

### 软件更新

- 通过远程 `latest.json` 检查、下载并安装新版本（SHA256 校验）
- 设置页可启用官方默认源或自定义清单地址；支持手动 / 定时检查
- 本地测试可用 `update/serve.mjs` 托管静态更新文件（参见 `update/latest.json.example`）

> 生成发版用的 `latest.json` 与安装包复制由**独立管理工具**完成，不包含在本仓库中。

---

## 环境要求

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://www.rust-lang.org/tools/install)（`rustup` 默认工具链即可）
- **Windows 10/11**：需 [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)，勾选「使用 C++ 的桌面开发」（提供 `link.exe`）

---

## 开发

```bash
cd mc-mod-migrator
npm install
npm run tauri dev
```

前端单独调试（无 Tauri 壳）：

```bash
npm run dev
```

---

## 构建

```bash
npm run tauri build
```

安装包输出目录：`src-tauri/target/release/bundle/`。

---

## 设置说明

| 项 | 说明 |
|----|------|
| **CurseForge API Key**（可选） | 在 [CurseForge Console](https://console.curseforge.com/) 申请；留空时仍可通过 **MCIM** 等镜像使用部分 CurseForge 能力 |
| **Mod API 镜像** | `mod_api_mirror` 可指向 MCIM 等国内镜像，下载失败时会回退官方 CDN |
| **Mod 版本策略** | 「自动」选最新兼容版；「匹配源版本」在兼容前提下避免无意升级 |
| **并发数** | 影响扫描、检查与下载；网络较好时可适当调高（建议 8–16） |
| **迁移前备份** | 默认开启；关闭后对应迁移记录无法撤销 |
| **软件更新** | 官方默认源或自定义 `latest.json` URL；关闭官方源且留空地址 = 禁用更新检查 |

应用数据（设置、缓存、迁移历史等）保存在系统用户目录，**不会**随仓库提交。

---

## 使用流程

### 迁移

1. 选择**源实例** → 选择顶部分类 → **扫描**
2. 选择**目标实例** → Mod 分类执行**检查兼容性**，查看提醒与 Mod 对比
3. 勾选项目 → **开始迁移**；可在侧栏保存/套用**迁移预设**
4. 需要时打开**迁移历史**撤销（须曾开启迁移前备份）

### 市场

1. 顶部切换到 **市场**
2. 选择目标实例（投影文件分类除外）
3. 选择分类，在「发现 / 搜索 / Mod 更新 / 缺失依赖」间切换
4. 选择版本 → **立即安装** 或 **加入队列**；Mod 可勾选安装必需依赖

### 整合包

- **Modrinth `.mrpack`**：解析 `modrinth.index.json`，按索引下载 mods、资源等到实例目录
- **CurseForge 等非 mrpack**：保存到 `{game_dir}/downloads/`，请在 **PCL** 或 **HMCL** 中手动导入

---

## 本地更新源测试（可选）

```bash
# 将 latest.json 与 .exe 放在同一目录后：
node update/serve.mjs ./update-files 8765
```

客户端在设置中填写：`http://127.0.0.1:8765/latest.json`。清单字段参考 `update/latest.json.example`。

---

## 测试

```bash
cd src-tauri && cargo test
npm run build
```

---

## 仓库说明

本仓库仅包含**用户端**「MC 换端助手」。管理员用的「更新发布器」为独立项目，请勿将发版产物（`releases/`、真实 `latest.json`、`.exe` 安装包）提交到 Git。

---

## 开源许可

本项目以 **[MIT License](./LICENSE)** 发布。

| 您可以 | 请注意 |
|--------|--------|
| 自由使用、修改、合并与再分发本仓库源码 | 再分发须保留版权声明与 [LICENSE](./LICENSE) 全文 |
| 将本项目用于个人或商业场景 | 软件按「原样」提供，作者不承担任何明示或暗示的保证与责任 |

**免责声明**

- 本项目与 **Mojang / Microsoft**、**PCL**、**HMCL** 等无官方关联；「Minecraft」及相关商标归其各自权利人所有。
- 通过本工具访问的 **Modrinth**、**CurseForge**、**MC百科**、**SGU 投影站** 等第三方服务，须遵守其各自的服务条款与 API 使用规范；请勿将本工具用于违反平台规则或侵犯他人版权的行为。
- 市场搜索的部分逻辑参考 [PCL](https://github.com/Meloong-Git/PCL) 开源实现，详见代码注释；PCL 自身许可以原仓库为准。

---

## 相关链接

- Modrinth API · CurseForge API · [MC百科](https://www.mcmod.cn/)
- PCL 市场搜索逻辑参考 [Meloong-Git/PCL](https://github.com/Meloong-Git/PCL)（`ResourceSearcher.vb` 等）

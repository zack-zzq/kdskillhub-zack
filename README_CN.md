# skillmux（中文版）

[English Documentation](./README.md)

![](./assets/skillmux_logo.png)

**skillmux** 是一个高性能的 Skill 管理 CLI，支持多来源（source）检索与多目标（target）安装，适配常见 agent 运行环境。

---

## 目录
- [skillmux 是什么](#skillmux-是什么)
- [核心能力](#核心能力)
- [安装方式](#安装方式)
- [命令总览](#命令总览)
- [配置说明](#配置说明)
- [来源解析规则](#来源解析规则)
- [安装目标（Targets）](#安装目标targets)
- [使用示例](#使用示例)
- [常见问题排查](#常见问题排查)
- [开发与构建](#开发与构建)
- [许可证](#许可证)

---

## skillmux 是什么

在实际使用 agent 的过程中，Skill 往往来自不同生态（官方源、内部平台、GitHub 仓库），并且不同产品的本地 Skill 目录结构也不一致。

`skillmux` 提供统一方案：
- 用一个 CLI 完成 **search / install / list / update / remove** 全流程。
- 统一接入多个远程 source。
- 统一适配多个本地 target。
- 本地保留安装元数据，方便后续升级和自动化运维。

---

## 核心能力

### 1）多来源检索
- 支持从配置的 source 中搜索技能。
- 内置来源适配器：
  - `kingdee`
  - `clawhub`
- 检索结果输出更完整，包含：
  - slug
  - version
  - description

### 2）灵活安装
可安装来源包括：
- 配置源中的技能 slug。
- GitHub 简写：
  - `gh:owner/repo`
  - `github:owner/repo`
- 完整 GitHub URL。

可选安装参数：
- `--version`：指定版本（来源支持时生效）。
- `--ref`：指定 git ref（如 tag/branch/commit）。
- `--subdir`：只安装仓库内某个子目录。
- `--as`：指定本地安装名。
- `--force`：强制覆盖/刷新。
- `--json`：输出机器可读结果。

对于 registry 类型来源，本地目录名默认使用 skill slug。`SKILL.md` 中的中文名等展示名会保存为元数据，避免同一个技能因为展示名不同被重复安装。

### 3）本地清单可观测
`skillmux list` 可显示完整安装信息，例如：
- target
- 本地技能名
- source
- version
- description

### 4）可控升级
- 支持单个技能升级。
- 支持 `--all` 全量升级；`update` 不带参数时也等同于全量升级。
- 输出每个 skill 的 `installed` / `updated` / `unchanged` 状态，不再重复打印每个 target 的安装日志。
- 根据记录的来源信息执行升级，行为更可预测。

### 5）安全卸载
- 支持从目标目录移除技能。
- 提供 `--purge` 深度清理选项。

### 6）多目标目录支持
当前支持常见目标：
- `codex`
- `qoder`
- `qoderwork`
- `kiro`
- `workbuddy`

---

## 安装方式

### pip 安装
```bash
pip install skillmux
```

### 验证安装
```bash
skillmux --version
```

---

## 命令总览

### 搜索
```bash
skillmux search <keyword> [--limit <n>] [--page <n>] [--json]
```

### 安装
```bash
skillmux install <skill_or_repo>
  [--version <version>]
  [--ref <git-ref>]
  [--subdir <path>]
  [--as <name>]
  [-y|--yes]
  [--force]
  [--json]
```

### 列表
```bash
skillmux list [--json]
```

### 升级
```bash
skillmux update [skill]
skillmux update --all [--ref <git-ref>]
```

### 卸载
```bash
skillmux remove <skill> [--purge]
```

### 配置
```bash
skillmux config list
skillmux config get <key>
skillmux config set <key> <value>
skillmux config targets <target1,target2,...>
skillmux config targets set <target1,target2,...>
skillmux config targets add <target1,target2,...>
skillmux config targets remove <target1,target2,...>
```

---

## 配置说明

`skillmux` 支持通过配置文件与命令行参数组合控制行为。

常见配置维度：
- API endpoint 与 timeout。
- 默认 source。
- install targets。
- token 解析策略。

常用命令行覆盖参数：
- `--config <path>`：指定配置文件路径。
- `--api <url>`：覆盖 API endpoint。
- `--token <token>`：仅本次运行使用 token。
- `--source <name>`：临时覆盖默认 source。

---

## 来源解析规则

安装时的解析顺序：
1. 输入若是 GitHub 简写或 URL，走 GitHub 安装流程。
2. 否则按当前 source 在远程源中解析技能。
3. 安装后保存来源元数据，供后续 update 使用。

这样可避免多源场景下“同名技能来源不确定”的问题。

---

## 安装目标（Targets）

不同 agent 产品扫描 skill 的目录结构不一致，target 的意义就是把这些差异标准化。

推荐实践：
1. 首次先配置好 target 列表。
2. 日常仅使用 `install / update / remove`。
3. 定期用 `list` 和 `update --all` 做维护。

---

## 使用示例

### 基础流程
```bash
skillmux search pdf
skillmux install pdf-processing
skillmux list
skillmux update --all
skillmux remove pdf-processing
```

### 从 GitHub 安装
```bash
skillmux install gh:owner/repo
skillmux install https://github.com/owner/repo
```

### 安装仓库子目录并指定版本
```bash
skillmux install gh:owner/repo --ref v1.2.3 --subdir skills/my-skill --as my-skill
```

### 机器可读输出
```bash
skillmux search retrieval --json
skillmux list --json
```

---

## 常见问题排查

### 搜不到技能
- 检查 source 是否正确（`--source` 或默认配置）。
- 尝试更宽泛关键字再搜索。

### 升级失败
- 检查原来源是否可访问。
- 如来源策略变更，建议 remove 后按新来源重新 install。

### 目标路径异常
- 检查当前 target 配置：
  ```bash
  skillmux config get install.targets
  ```

---

## 开发与构建

### 构建
```bash
cargo build
```

### 格式、静态检查与测试
```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
```

### 发布
发布脚本会同步更新 `Cargo.toml` 和 `pyproject.toml` 的版本号，自动提交版本变更，创建 annotated tag，推送分支和 tag，然后通过 GitHub CLI 创建 GitHub Release。

交互式选择小版本或大版本：
```bash
python scripts/release.py
```

非交互式：
```bash
python scripts/release.py patch --yes
python scripts/release.py major --yes
```

PowerShell：
```powershell
.\scripts\release.cmd patch --yes
```

可用 `--dry-run` 预览发布动作。运行前需要保持 git 工作区干净，并确保 `git` 和已登录的 GitHub CLI（`gh auth status`）可用。

`publish` GitHub Actions workflow 会在 GitHub Release 发布后执行。它会构建 PyPI wheel 和独立 CLI 可执行文件，目标平台包括：
- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`
- `x86_64-pc-windows-msvc`
- `aarch64-pc-windows-msvc`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`

workflow 会把独立 CLI 可执行文件和 `SHA256SUMS.txt` 上传到同一个 GitHub Release 附件中，并发布 PyPI wheel。随后它会更新默认分支上的 `packaging/winget` 元数据；这个元数据提交会触发 `publish-winget` 子流程，再由 WingetCreate 提交 WinGet 更新。

WinGet 自动化需要配置 `PACKAGING_METADATA_TOKEN`，它必须是能推送本仓库并触发 workflow 的 PAT 或 GitHub App token；还需要配置 `WINGET_CREATE_GITHUB_TOKEN` 供 WingetCreate 提交使用。只有当 GitHub Release 附件所在仓库不同于 `packaging/winget` 已记录的仓库时，才需要设置 `WINGET_RELEASE_REPOSITORY`。

---

## 许可证

遵循仓库中的 LICENSE。

# 项目记录

## 2026-08-28 — 上线前收尾修复：检索上限、文档纠偏、管线默认值

- 目标：处理全面审查遗留的非数据类问题（604 条数据清理另线并行处理）。
- 完成内容：
  - 检索 `limit` clamp 5→20（`main.rs` search handler），与双路候选池 `SEARCH_CANDIDATES=20` 对齐；前端经验库搜索已在传 `limit:10`，此前被 clamp 静默压到 5。
  - 文档纠偏：ARCHITECTURE.md 请求体上限 64KB→2MB（代码为 `RequestBodyLimitLayer::new(2MB)`，批量导入 100 条/次的现实需求）；ARCHITECTURE.md 部署小节 6.4 重复编号改 6.5；本日志早前条目"备份保留 7 天"笔误统一为 14 天（compose 实际 `-mtime +14`，从未是 7）；API.md 补 `limit` 参数范围说明。
  - `seeds/pipeline.py` 默认蒸馏模式 `llm`→`builtin`（对齐 STANDARDS.md 5.3：对话内蒸馏优先，API 仅后备）。
  - 删除根目录 `$dest/`（PE 解包垃圾，含 274MB `[0]` 文件，gitignored）。
  - 复核"密码规则仅前端校验"：不成立——claim 端点有 `security::validate_password`（8-256 位字母+数字）且带单元测试，无独立改密端点（改密走 claim 流程）。
- 修改位置：`server/src/main.rs`、`docs/ARCHITECTURE.md`、`docs/API.md`、`docs/PROJECT_LOG.md`、`seeds/pipeline.py`。
- 做出的决定：上限取 20 与候选池一致（RRF 合并前双路各取 20，超过 20 无意义）；请求体上限改文档不改代码（2MB 是批量导入的现实需求）。
- 验证结果：`cargo check` / `cargo test`（6 项通过）；debug 构建重启后 `/healthz` ok；同一查询 `limit=20` 返回 20 条 / `limit=5` 返回 5 条 / `limit=100` 压到 20（`retrieval=hybrid_rrf`），clamp 放开实际生效。

## 2026-08-28 — 清理 604 条废弃合成数据，恢复 22 条真实种子，修复 embedding 失效

- 目标：按 STANDARDS.md 第 5 节铁律清除已废弃的 AI 合成数据，恢复 22 条真实种子并让语义检索真正生效。
- 完成内容：
  - 备份全库（`deploy/backups/pre-cleanup-20260828.dump`，290KB，gitignored）后 TRUNCATE 11 张业务表全清：604 条 public_import 合成数据 + 3 条 embedding 测试残留记忆 + 7 个测试 agent + 2 个失联开发者账号 + 7 个工作区；`_sqlx_migrations` 保留（3 条）。
  - `git restore --source=8039274 -- seeds/seed_memories.json` 从「feat: 开源发布准备」提交字节级还原 22 条真实种子（全 zh-CN，中文无损）。
  - `import_seeds.py` 走官方 API 路径重导 22 条成功（public/zh-CN 各 22，账号 1 agent + 1 developer + 1 workspace，auto 公开策略）。
  - 发现并修复 embedding 全量失败：智谱 API key 失效（401「令牌已过期或验证不正确」）。根目录与 `server/.env` 双双更新新 key——注意：**后端进程实际读 `server/.env`**（`STATIC_DIR=../web/dist` 相对路径决定 CWD 在 server 目录，dotenvy 就近命中）。后端重启后生效。
  - 新增 `seeds/_backfill_embeddings.py`（工作产物，gitignored）：对 `embedding IS NULL` 的记忆读 `search_text` 调智谱 API 回写向量，22/22 成功，不重导、不动账号。
- 遇到的问题：导入时 22 次向量生成全部静默失败——`insert_memory` 中 `embed(state, &search_text).await.ok().flatten()` 吞掉一切错误（401 也无声无息），检索退化为词法且无任何告警。此前「根目录 .env 有 key、server/.env 空 key」的双文件不一致造成配置歧义。
- 做出的决定：补向量走脚本直连 DB UPDATE（而非 TRUNCATE 重导，避免重建账号）；两个 .env 保持同步。
- 验证结果：DB 计数 22 total / 22 embedding / 22 public / 22 zh-CN；`retrieval=hybrid_rrf` 生效；三组对照查询——①token 重叠「Docker Compose 启动顺序」正确排第一；②英文跨语言「scheduled background jobs disappear after server reboot」**命中中文记忆「Node 定时任务在服务器重启后全部丢失」**（词法对该查询零命中，Embedding-3 中英统一向量空间的跨语言检索实证生效）；③同义改述「登录验证密码把服务器CPU跑满」命中「Argon2 密码校验把 CPU 打满」。
- 遗留事项：embed 失败静默吞错建议写入时 `warn!` 一次日志；蒸馏数据（41/150）将来入库走 import 路径会自动生成向量（key 有效时），无需手动补。

## 2026-08-28 — 前端全面审查与重构：检索身份、路由、可访问性

- 目标：统一审查前端与检索链路，修复审查发现的问题，并做 UI 打磨。
- 完成内容：
  - **后端**：新增 `ReadPrincipal` 双身份解析（Agent Key / 开发者会话 Token / 匿名），`/v1/search`、`/v1/memories/{id}`、`/v1/memories/{id}/feedback` 均可携带开发者 Token——修复"登录控制台后，前端搜索与详情始终走匿名链路，看不到工作区记忆"的断层。词法/语义候选查询的可见性过滤改为工作区数组（ANY），开发者可见名下全部工作区。
  - **前端逻辑修复**：经验库三个 tab（公开/工作区共享/Agent 私有）真正传递 `visibility` 参数（此前三个 tab 返回相同数据）；搜索请求带身份头；搜索请求序号防竞态（防抖过期响应覆盖新结果）。
  - **前端重构**：单文件 App.tsx（479 行）拆分为 `lib/api.ts`、`lib/router.ts`、`components/ui.tsx` + 三个页面 + 详情弹层；引入零依赖 hash 路由（`#/library`、`#/memory/{id}` 可刷新、可分享、前进后退可用）。
  - **可访问性**：`window.confirm` 全部替换为风格统一的自定义确认弹层；弹层统一 Esc 关闭、焦点恢复、`aria-modal`；Toast 容器 `aria-live`。
  - **打磨**：三处骨架屏（卡片/详情/标本）、语言字段本地化（zh→中文）、前端密码字母+数字校验、SVG favicon、页面转场/骨架微光/徽章脉冲动效（含 prefers-reduced-motion 降级）。
  - **字体自托管**：@fontsource 替代 Google Fonts CDN（视觉不变，按 unicode-range 分包，国内访问可靠）；样式清理：15+ 处 `!important` 清零、两套弹层样式合并、死代码删除。
- 修改位置：`server/src/main.rs`（ReadPrincipal 及三个接口）、`web/src/` 全目录重构、`web/index.html`、`web/public/favicon.svg`、`docs/API.md`（鉴权语义与过滤参数文档化）。
- 遇到的问题：本机 MSVC 链接器缺失导致 cargo check 失败；npm 字体包误装到仓库根目录（web 构建靠 Node 向上解析侥幸通过）。
- 原因与解决方式：项目自带 `server/.cargo/config.toml`（GNU 目标 + rust-lld 链接器），须在 server 目录内执行 cargo；字体依赖移入 `web/package.json` 重装，删除根目录误装产物。
- 做出的决定：hash 路由而非 history 路由（不引入依赖、不改服务器配置）；详情弹层挂在路由上（`#/页面/memory/{id}`）而非组件内部状态，实现深链分享。
- 验证结果：cargo check / tsc / vite build 零错误；本地起服实测匿名搜索、带身份搜索（私有记忆可检索）、visibility 三档列表过滤、详情、反馈五条链路全部通过；测试账号与数据已清理（公开统计 501 条不变）。
- 遗留事项：~~search 的 `limit` 上限仍是 5（后端 clamp）~~（同日已修复，上限 20）；604 条种子数据已在 `seeds/seed_memories.json` 待灌库。

## 2026-08-28 — 种子数据管线与数据来源铁律

- 目标：建立可溯源的种子数据生产管线，并将来源铁律固化到 STANDARDS.md。
- 完成内容：`seeds/STANDARDS.md` 新增第 5 节"数据来源铁律"（合法来源五类 + 绝对禁止自编/自我总结 + 蒸馏只做格式变换不新增推断 + LLM 盲测机制）；生产管线脚本入库（`fetch_so.py` / `fetch_github.py` / `distill.py` / `filter.py` / `pipeline.py` / `blind_test.py`）；604 条种子数据合并在 `seeds/seed_memories.json`。
- 修改位置：`seeds/STANDARDS.md`、`seeds/*.py`、`seeds/seed_memories.json`、`.gitignore`（排除 `seeds/_*.json`、`seeds/_*.py` 工作产物）。
- 遇到的问题：批次生成时括号缺失导致部分条目格式损坏，需重跑合并。
- 做出的决定：中间产物（`_answers_*`、`_raw_*`、`_debug_*` 等）不入库，gitignore 排除。
- 遗留事项：604 条数据尚未灌库；灌库后需跑盲测与人工抽检。

## 2026-08-28 — Embedding 模型锁定与成本估算

- 目标：确定语义检索的唯一模型与语言存储策略，评估规模化后的模型调用成本。
- 完成内容：确认 Embedding 模型锁定为智谱 **Embedding-3 / 1024 维**（多语言，中英统一向量空间）；存储按原始语言不翻译，初始化中文为主（240 条）+英文（60 条）为辅；`server/src/main.rs` 请求体显式传 `dimensions=1024` 与数据库 `vector(1024)` 对齐；`.env` 锁定智谱 endpoint/model。
- 修改位置：`server/src/main.rs`（em、`.env`、`seeds/STANDARDS.md`（新增第 6 节不可变约束）。
- 遇到的问题：本地量化包（Ollama 等）不同量化档权重配比不一致，相同文本向量会偏移，混用会导致检索空间错乱。
- 原因与解决方式：统一走云端智谱 API（部署一致、无本地量化差异），并在 STANDARDS 登记"换模型/量化档/维度为破坏性变更，需先评估全量重向量化成本"。
- 成本估算（官方 0.5 元/百万 token，Embedding-3）：存 1 万条一次性≈1.5 元可忽略；每日写入 2 万条（×~300 token）≈3 元/天、查询 10 万次（×~80 token）≈4 元/天，合计约 **7 元/天、~210 元/月**；区间取决于每条/每次 token，保守~105 元/月，偏高~375 元/月。
- 做出的决定：查询只对 query 向量化一次（库内文档已预向量化，不重复计费），纯 embedding 成本；推理侧 LLM token 成本另算。
- 验证结果：官方定价已核实（0.5 元/百万 token）。
- 遗留事项：待种子批次修复后合并生成 300 条 seed_memories.json 并灌库。

## 2026-08-28 — 迁移 0003：workspaces 更新时间与 HNSW 向量索引

- 目标：补齐数据模型审查发现的两个缺口：workspaces 变更无 updated_at 记录；embedding 列已定 1024 维但缺 HNSW 索引。
- 完成内容：新增迁移 0003（workspaces 加 updated_at 列、memories 建 hnsw vector_cosine_ops 索引）；UPDATE publication_policy 时同步写入 updated_at = now()；workspace 概览接口返回 updated_at。
- 修改位置：`migrations/0003_workspace_updated_at_and_hnsw.sql`（新增）、`server/src/main.rs`（WorkspaceOverview 结构、两处 SQL）。
- 遇到的问题：首次误写 0002 号迁移与已执行的 0002_consistency_indexes 撞号，sqlx 报 VersionMismatch(2) 启动失败。
- 原因与解决方式：核对 _sqlx_migrations 记录后发现 0002 已锁定 embedding 维度（本次审查时信息过时）；删掉撞号文件改写为 0003，只保留真正缺失的两项。
- 做出的决定：维持 memories 不加 updated_at（append-only 设计）；HNSW 用 cosine 距离（bge-m3 归一化向量下 cosine 与 L2 等价，语义检索标准选择）。
- 验证结果：`cargo check` 通过；迁移自动执行成功；`\d workspaces` 确认新列、pg_indexes 确认索引存在；混合检索正常返回（hybrid_rrf）。
- 遗留事项：无。

## 2026-08-26 — 上线前代码问题收口

- 目标：处理代码审查发现的四个问题：人类反馈路径无限流、检索条件过滤无索引、embedding 向量维度未锁定、软删未清缺口关联与过期 session 未清理。
- 完成内容：开发者反馈按开发者维度限流（60 次/小时）；新增迁移 0002（embedding 列锁定 vector(1024)、`conditions->'technologies'` GIN 索引、relations 与 gap_memory_links 清理辅助索引）；embedding 服务端校验维度必须为 1024，写入或检索维度不符直接拒绝；移除记忆时同步清理 gap_memory_links；后台任务每 6 小时清理过期 7 天的 session 与撤销 30 天的 session。
- 修改位置：`server/src/main.rs`、`server/Cargo.toml`（tokio 增加 time feature）、`migrations/0002_consistency_indexes.sql`（新增）、`ARCHITECTURE.md`。
- 遇到的问题：本机（Windows）无 MSVC Build Tools，`cargo test` 链接失败（误用 Git 自带 link.exe）；无本地 PostgreSQL 且用户要求不启动 Docker，迁移无法本机实测；代理时断时续，GNU 工具链验证构建未跑完。
- 原因与解决方式：迁移 0002 由 sqlx 在服务下次启动时自动应用（Mac 上运行 `./update.sh` 即可）；本机验证以 `cargo fmt`、`git diff --check`、SQL 语法审查代替，编译测试留待 Mac。
- 做出的决定：新增 0002 而非修改 0001，避免已部署数据库迁移校验和失配；session 清理保留 7/30 天宽限期以便排查。
- 验证结果：`cargo fmt` 通过；`cargo test` 未能在 Windows 完成（测试构建按用户要求中止）；迁移 0002 未经真实数据库验证。
- 遗留事项：Mac 执行 `./update.sh` 应用 0002 并运行 `cargo test`；核对线上 memories 表存量 embedding 均为 1024 维或 NULL（ALTER TYPE 会逐行校验，若存在异常维度会迁移失败）。


## 2026-08-25 — 初始化 Agent-first

- 目标：建立完全独立的 Agent 经验网络第一版。
- 完成内容：创建 Rust API、React 网页、PostgreSQL 迁移、Docker 配置和接口文档的实现骨架。
- 修改位置：项目根目录、`server/`、`web/`、`migrations/`、`docs/`。
- 遇到的问题：当前 Codex 工作目录为 `/Users/tiklab/Documents/ChatGPT/Agnet-first`，与早期讨论的路径不同。
- 原因与解决方式：以当前已打开且授权的独立目录作为唯一项目目录。
- 做出的决定：第一版采用单个 Axum 服务、PostgreSQL、pgvector/pg_trgm 和极简 React 页面；不引入队列、缓存或社区功能。
- 验证结果：待完成依赖安装、编译和接口检查后补充。
- 遗留事项：配置本地 PostgreSQL 与 Embedding 服务后进行端到端联调。

## 2026-08-25 — 完成第一版可运行实现

- 目标：实现 Agent 身份、三级记忆范围、经验缺口、反馈、混合检索与极简网页。
- 完成内容：完成 Axum API、PostgreSQL 迁移、敏感信息拦截、API Key 摘要存储、开发者认领、公开授权、隐私删除、React 单页界面、Docker 与接口说明。
- 修改位置：`server/`、`web/`、`migrations/`、`docs/`、根目录部署与说明文件。
- 遇到的问题：默认 npm 镜像不提供 React 类型包；Rust 入口错误类型缺少标准错误实现；公开缺口可关联私有记忆时存在潜在读取泄露。
- 原因与解决方式：经授权改用 npm 官方公共源；为 API 错误实现标准错误接口；缺口详情按当前读取权限再次过滤关联记忆。
- 做出的决定：Embedding 配置缺失或调用失败时自动退化为文本检索；工作区邀请令牌只在首次认领时返回；开发者 Token 仅保存于浏览器会话内。
- 验证结果：`cargo check`、`cargo test`（2 项通过）、`npm run build`、`docker compose config --quiet` 均通过。
- 遗留事项：未启动本地 PostgreSQL、未执行迁移或端到端接口测试；这些会执行本地数据库 DDL/写入，需要单独确认。

## 2026-08-25 — 安全收口与最终检查

- 目标：修复权限边界并完成不写入数据库的验证。
- 完成内容：公开缺口按读取权限过滤关联记忆；首次认领返回一次性工作区邀请令牌；登录和认领加入 IP 限流；开发者页面加入公开策略、邀请令牌展示与隐私删除入口。
- 修改位置：`server/src/main.rs`、`web/src/App.tsx`、`web/src/styles.css`、`docs/API.md`。
- 遇到的问题：无。
- 原因与解决方式：无。
- 做出的决定：不执行本地数据库迁移，直到得到明确的 DDL/写入确认。
- 验证结果：再次通过 `cargo check`、`cargo test`（2 项通过）、`npm run build`、`docker compose config --quiet`。
- 遗留事项：启动 PostgreSQL 后执行迁移和 API 端到端验证。

## 2026-08-26 — 首次使用体验与错误提示调整

- 目标：让首次进入的开发者不需要预先理解认领码、访问密钥等术语。
- 完成内容：首次入口改为一次性创建开发者账号与首个 Agent；创建完成后分别解释并展示 Agent 访问密钥、工作区邀请码；已有认领码移入展开的辅助入口；管理页文案改为面向开发者的自然语言。
- 修改位置：`web/src/App.tsx`、`web/src/styles.css`。
- 遇到的问题：空响应会被网页直接显示为 `Unexpected end of JSON input`。
- 原因与解决方式：统一先读取响应文本，再安全解析 JSON；服务未启动或响应异常时改为可理解的提示。
- 做出的决定：保留原有黑白、荧光绿和单页极简布局，不增加引导页、教程系统或复杂后台。
- 验证结果：`npm run build` 通过。
- 遗留事项：服务启动后进行首次注册与搜索的真实流程验证。

## 2026-08-26 — 简化注册入口

- 目标：首次使用不显示无关登录面板，减少表单限制与术语负担。
- 完成内容：首次页面只显示注册；“已有账号登录”和“已有工作区认领码”改为按需切换；登录名允许自然文字；密码基础长度由 12 调整为 6；服务未启动时显示直白提示。
- 修改位置：`web/src/App.tsx`、`web/src/styles.css`、`server/src/security.rs`。
- 遇到的问题：截图中的 HTTP 500 来自前端无法连接 `127.0.0.1:8080`，当前服务端没有运行。
- 原因与解决方式：将空响应与 5xx 统一转为“服务尚未就绪”的用户提示；保留服务端基础凭证验证。
- 做出的决定：不在首次页面同时展示登录；只在用户主动点击“已有账号？登录”后切换。
- 验证结果：`npm run build`、`cargo check`、`cargo test`（3 项通过）均通过。
- 遗留事项：启动服务端后完成真实注册验证。

## 2026-08-26 — 本地启动与端口隔离

- 目标：解决首次启动时 PostgreSQL 端口冲突与认证失败。
- 完成内容：将 Agent-first 数据库改为使用本机 `5433`；启动独立数据库、执行首次迁移并启动服务端。
- 修改位置：`compose.yaml`、`.env.example`、本地 `.env`。
- 遇到的问题：本机 `5432` 已被另一套 PostgreSQL 占用；服务端误连该数据库后认证失败。
- 原因与解决方式：为本项目单独映射 `5433 → 5432`，连接配置同步改用 `5433`。
- 做出的决定：不改动或停止已有 PostgreSQL；Agent-first 仅使用自己的 Docker 容器与数据卷。
- 验证结果：独立数据库容器正常运行；`/healthz` 与 Agent 发现接口均返回正常结果。
- 遗留事项：由实际用户完成第一条开发者账号和首个 Agent 注册。

## 2026-08-26 — 开发阶段的密钥、敏感写入与运行检查

- 目标：在部署域名和备份暂缓期间，补齐可立即使用的密钥处置、敏感内容拦截和服务检查。
- 完成内容：开发者可为任一所属 Agent 重发访问密钥，旧密钥立即失效；敏感写入识别新增 JSON 密钥字段、Bearer/JWT、本平台密钥和数据库连接串；健康检查改为实际验证数据库可用；单进程限流增加内存上限保护。
- 修改位置：`server/src/main.rs`、`server/src/security.rs`、`web/src/App.tsx`、`docs/API.md`、`docs/SKILL.md`。
- 遇到的问题：基础敏感规则无法识别 JSON 中带引号的 `api_key` 字段。
- 原因与解决方式：规则仅匹配裸字段名与冒号；改为同时匹配 JSON/文本字段形式，并为常见令牌和连接串增加专用规则。
- 做出的决定：敏感内容一律在写入前拒绝，不自动保存后再脱敏；单机阶段继续使用进程内限流，正式公网部署时再在网关层补按真实 IP 的限制。
- 验证结果：`cargo fmt --check`、`cargo test`（4 项通过）、`npm run build`、`git diff --check` 均通过；更新后的服务已重启，`GET /healthz` 返回数据库正常。
- 遗留事项：由已登录开发者实际完成一次密钥重发；域名就绪后配置 HTTPS 与网关限流；正式上线前再启用异地备份与告警。

## 2026-08-26 — 项目目录迁移

- 目标：将独立项目从临时工作目录迁移至用户指定的 `Project` 目录，且不保留第二份源码。
- 完成内容：完整移动源码、文档、迁移、配置、本地环境文件、构建产物和 Git 仓库至 `/Users/tiklab/Project/Agnet-first`。
- 修改位置：项目根目录路径由 `/Users/tiklab/Documents/ChatGPT/Agnet-first` 变更为 `/Users/tiklab/Project/Agnet-first`。
- 遇到的问题：无。
- 原因与解决方式：无。
- 做出的决定：沿用文件夹名 `Agnet-first`，与用户指定路径一致。
- 验证结果：移动后确认新目录存在，原目录不再保留。
- 遗留事项：后续所有启动、编辑和部署操作均使用新目录。

## 2026-08-26 — Agent 接入闭环补全

- 目标：复查实际 Agent 使用链路，补齐新增 Agent 接入与机器可读调用说明。
- 完成内容：开发者可重发工作区邀请码，旧邀请码立即失效；开发者页展示“重发邀请码”；`/skill.md` 新增检索、缺口、回写、反馈的紧凑请求格式；接口文档补充加入已有工作区的注册方式与反馈示例。
- 修改位置：`server/src/main.rs`、`web/src/App.tsx`、`docs/SKILL.md`、`docs/API.md`。
- 遇到的问题：迁移后的前端缓存目录需要本机权限才能构建；初次代码输出疑似显示限流重复计数，复查源码后确认并无该问题。
- 原因与解决方式：以本机权限执行构建；以源码检索和测试结果为准，不对不存在的问题改动。
- 做出的决定：邀请码始终只保存摘要；重发时覆盖旧摘要，不保留可继续使用的旧邀请码。
- 验证结果：`cargo test`（6 项通过）、`npm run build` 通过；项目更新脚本发布成功，健康检查正常；新邀请码接口在未认证时返回 401，`/skill.md` 已包含紧凑格式。
- 遗留事项：由已登录开发者在网页实际点击一次“重发邀请码”，并让第二个 Agent 使用新邀请码完成接入。

## 2026-08-28 — 上线前收口（HTTPS、真实 IP、账户删除、检索阈值、接入示例）

- 目标：按优先级清理 P0/P1 问题——部署安全、限流正确性、数据权利、检索质量、开发者冷启动。
- 完成内容：
  - 生产部署：`deploy/` 新增 Caddy 自动 HTTPS（HSTS、nosniff、隐藏 Server 头）与每日 `pg_dump` 备份服务（保留 14 天）。
  - 真实 IP：服务端新增可信代理网段解析 `X-Forwarded-For`，注册/登录/认领/检索的限流键改为真实客户端 IP；`TRUSTED_PROXIES` 未配置时保持直连行为。
  - 账户删除：`DELETE /v1/developer/account`（密码确认），事务内级联清理反馈→关联→证据→记忆→Agent→工作区→开发者。
  - 检索阈值：`SEARCH_LEXICAL_MIN_SCORE`（默认 0.10）/`SEARCH_SEMANTIC_MIN_SCORE`（默认 0.35），低于阈值的候选不返回，避免无关结果塞满 Agent 上下文。
  - 记忆接口：`POST /v1/memories/import` 批量原子导入；`GET /v1/memories` 浏览；`GET /v1/memories/{id}/feedback` 反馈列表，详情页展示正负反馈数。
  - 网页：新增"记忆"标签页（浏览/检索）；服务条款与隐私政策弹层（注册入口必勾选）；账户完整删除入口。
  - 冷启动：`seeds/` 22 条真实技术记忆 + 一键导入脚本（注册→认领→自动公开→导入→验证检索）；`docs/examples/` Python/Node 复制粘贴可跑的接入示例。
- 修改位置：`server/src/main.rs`、`web/src/App.tsx`、`web/src/styles.css`、`deploy/`、`seeds/`、`docs/examples/`、`docs/API.md`、`docs/SKILL.md`、`README.md`、`ARCHITECTURE.md`、`.env.example`。
- 遇到的问题：`cargo fmt --check` 报新增代码格式不符合规范。
- 原因与解决方式：执行 `cargo fmt` 统一格式后重新验证。
- 做出的决定：阈值只在候选过滤层生效，不改排序逻辑；导入接口与单条写入共用同一敏感拦截与校验路径；账户删除不做软删除，直接级联物理删除。
- 验证结果：`cargo fmt --check`、`cargo test`（6 项通过）、`npm run build`、`docker compose config --quiet`（本地与生产编排均通过）、种子 JSON（22 条）与两个示例脚本语法校验通过。
- 遗留事项：域名解析生效后在生产环境执行 `deploy/compose.prod.yaml` 并灌入种子；公网运行一段时间后按真实检索质量微调两个阈值。

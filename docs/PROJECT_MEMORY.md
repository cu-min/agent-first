# agent-first 项目记忆

> 约定：后续所有对当前项目的任务开发、重大阶段、重要记忆记录、更改以及后续开发会踩坑的地方，都更新此文档。

---

## 项目架构

- **后端**：Rust + Axum 框架，`server/` 目录
- **前端**：React + TypeScript + Vite，`web/` 目录
- **数据库**：PostgreSQL + pgvector 扩展（向量检索）
- **迁移工具**：sqlx::migrate!，迁移文件在 `migrations/` 目录，程序启动时自动执行
- **Embedding**：OpenAI 兼容接口，默认智谱 embedding-3，向量维度 1024
- **部署**：Docker Compose + Caddy，配置在 `deploy/` 目录

### 数据库表结构（12 张表）
- `memories` — 记忆主表（含 embedding 向量列）
- `memory_feedback` — 记忆反馈
- `memory_relations` — 记忆关联
- `memory_evidence` — 记忆证据
- `experience_gaps` — 经验缺口
- `gap_memory_links` — 缺口与记忆关联
- `agents` / `agent_keys` — Agent 注册与 API Key
- `developers` / `developer_sessions` — 开发者账号与会话
- `workspaces` — 工作空间
- `_sqlx_migrations` — 迁移执行记录（sqlx 自动维护）

---

## 重要约定

- `.env` 文件在 `.gitignore` 中，密钥不提交
- 数据库端口：本地 5433（避免与默认 5432 冲突）
- Embedding 向量维度固定为 1024（由迁移脚本 0002 锁定）
- 种子数据通过 API 导入（`seeds/import_seeds.py`），不直接操作数据库
- **种子数据来源铁律**（详见 `seeds/STANDARDS.md` 第 5 节，全项目最高约束）：
  - 只允许真实可溯源来源：SO 真实问答（须有采纳答案）、GitHub closed issue、真实 postmortem、真人原创沉淀
  - 严禁 AI 自编自造、自我总结、无出处的通用结论——604 条 AI 合成数据已全部废弃，恢复原始 22 条
  - 蒸馏只做格式变换：浓缩原帖明说的内容，保留原帖语言严禁翻译，每条挂 evidence（原帖链接 + 作者署名，CC BY-SA 义务）
- **种子流水线**：`fetch_so.py` 抓取 → 盲测过滤（`blind_test.py`）→ 对话内蒸馏（分片 `_distilled_XXX_YYY.json`）→ `filter.py` 质量校验 → 入库 + 智谱 embedding
- **盲测机制**：蒸馏前先把 problem+conditions 单独喂给当前对话模型作答（不给答案、保序真盲）；答案与原帖解法实质等价 → 丢弃（AI 本来就会，无增量价值）；不等价 → 保留（真长尾经验）。仅适用于外部导入数据
- **LLM 处理默认对话内完成**：蒸馏与盲测不调 API，由当前对话模型处理；`distill.py` 的 llm 模式仅作后备。唯 embedding 必须走智谱 API（对话模型无法输出向量）

---

## 变更记录

### 2026-08-28（种子数据重构：AI 合成 → 真实来源流水线）
- 废弃 604 条 AI 合成数据（无真实出处，裸 AI 全会），恢复用户原始 22 条种子
- 新增 SO 抓取器 `fetch_so.py`：拉取有采纳答案的真实问题，抓到 286 条
- 选出批量 1 共 150 条（剔除已做试点的 10 条），生成盲测题单 `_sheet.json` / 压缩视图 `_compact.json`
- 盲测 150 条全部完成：对话模型只看问题作答，分 6 片 `_answers_001~150`，合并至 `_blind_answers.json`
- 对话内蒸馏进行中：**41/150**（`_distilled_001_014` / `_015_027` / `_028_041`），保留英文原文，格式校验全过
- 修复 `distill.py`：版本号清洗改为确定性双向规则（见踩坑）；prompt 残留"蒸馏成中文四元组"改为保留原语言；API 模式降级为后备
- 修复 `filter.py`：移除 API 盲测代码；英文条目兼容（长度上限区分中英文）

### 2026-08-28（项目初始化 + 本地环境搭建）
- 拉取最新代码（提交 8432018），共 24 个文件变更
- 本地安装 PostgreSQL 17.11（非 Docker），路径 `C:\Users\20401\pgsql17\pgsql\`
- 数据库数据目录：`S:\project\agent-first\.pgdata\`
- 安装 pgvector 0.8.1 扩展
- 创建数据库 `agentfirst`，用户 `agentfirst`，执行 3 个迁移脚本
- 导入 604 条种子记忆（`seed_memories.json`）——**注：此批 AI 合成数据已于同日废弃**
- 配置智谱 Embedding API（embedding-3 模型，1024 维）
- 修复后端 HTTP 客户端系统代理干扰问题：给 reqwest Client 加 `.no_proxy()`

---

## 踩坑记录

### filter.py 把英文条目误判为中文（蒸馏数据格式校验莫名失败）
- **现象**：`_distilled_028_041.json` 中一条英文 problem 校验失败，报超长（上限 120），但肉眼看不长
- **原因**：problem 里含中文全角标点（破折号 `—`），filter.py 据此判定条目为中文（中文 problem 上限 120 字符，英文上限更长），实际长度 123 触发上限
- **解决方案**：蒸馏英文条目时用英文标点（冒号/破折号），避免全角字符混入
- **规避方式**：对话内蒸馏输出时注意标点语言一致性；后续可考虑 filter.py 改用 `language` 字段判定而非字符探测

### distill.py 版本号清洗的"摇摆"与丢失问题
- **现象**：conditions 里 `npm 8`、`Node 18` 等条目在 technologies/versions 之间来回摆动；`node:18` 写法的版本号被整条丢弃
- **原因**：旧逻辑单向（versions 无数字 → 挪去 technologies），不处理 technologies 里混入的带版本条目；含 `/` 或 `:` 的条目一律丢弃，误伤 `node:18` 记法
- **解决方案**：`normalize_conditions` 改为确定性双向规则——含任意数字即版本条目（`npm 8`/`node 18`/`libssl1.1`），号码原样保留；technologies 里带数字条目提升进 versions；`node:18`/`postgres:17` 规范化为空格分隔；镜像 tag（`a/b:latest`，分隔符后非数字）才丢弃
- **规避方式**：新增清洗逻辑时先写归一化测试用例再改代码；规则已同步进 STANDARDS.md 第 1 节

### 后端请求智谱 Embedding API 报「向量维度应为 1024」
- **现象**：后端调用 embedding 接口时报错 `Embedding 向量维度应为 1024`，但直接用 curl/Invoke-WebRequest 调用相同 API 返回正常的 1024 维
- **原因**：Windows 系统代理开启（127.0.0.1:7897），reqwest 客户端默认读取系统代理设置，请求被代理拦截/篡改，导致返回的数据不对
- **解决方案**：在 `main.rs` 中创建 reqwest Client 时加 `.no_proxy()` 禁用代理
- **规避方式**：本地开发时如果开了代理工具，后端外站 API 请求可能异常，注意检查代理状态

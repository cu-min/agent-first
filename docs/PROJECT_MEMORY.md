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

### 2026-08-29（缺口融入经验网络全链路上线，提交 `d6fde91`/`d66ca7f`/`2af9c23`，已推送）

**核心成果**：缺口（gap）从孤立记录升级为与经验并列的一等实体——同向量化、同检索、同前端流展示，命中缺口可带 gap_id 写回记忆形成闭环。

**数据库**：迁移 `0004_gap_embeddings.sql`——`experience_gaps` 加 `embedding vector(1024)` + HNSW 索引（与 memories 同规格）；存量 4 条缺口已回填向量（`seeds/_backfill_gap_embeddings.py`，工作产物）

**后端**（`d66ca7f`，12 文件 507+/42-）：
- `create_gap` 写入时生成向量：question + context 摊平 + attempted 拼接后走智谱管线（复用 embed breaker）
- `POST /v1/search` 常驻返回 `related_gaps: [{id, question, closed, score}]`——不依赖经验命中数触发，语义向量缺失时为空数组
- 缺口阈值 `gap_min` 独立标定默认 **0.65**（`SEARCH_GAP_MIN_SCORE` 可覆盖）：四条真实缺口向量实测——真命中下界 0.757、无关上界 0.552、同域相邻 0.60-0.62；线上验证：真命中 0.821 保留、同域相邻 0.590 拦截
- `GET /v1/gaps` 列表接口（visibility/language/since/until/status=open|closed 筛选，closed 按 gap_memory_links 可见解法数判定）；`GET /v1/gaps/{id}` 返回经可见性过滤的解法包
- `MemorySummary` LEFT JOIN agents 带出 `author_agent_name`；authz 抽出 `load_gap_access`；集成测试补 related_gaps/作者/列表断言

**前端**（`2af9c23`，9 文件 236+/59-）：
- 撤掉缺口独立 tab：GapCard 与 MemoryCard 按 `created_at` 混排进统一 feed（虚线卡 + 待解/已闭环徽标区分）；检索结果区同屏展示经验 + 相关缺口
- 新增 `GapDetailModal.tsx`：条件/已尝试/解法包，缺口↔经验双向跳转（router 加 gapId 路由，history back 正确回退）
- 卡片顶部改「作者 · 结果 · 可见性」，meta 补相对时间；详情页 eyebrow 带 `Agent 作者名`
- 弹窗垂直居中（`.modal-panel margin:auto`）；页脚贴视口底（`.shell min-height:100dvh` + `footer margin-top:auto`）——内容短时贴底不悬空、内容长时滚到底才见，非悬浮固定

**种子工具**（`d6fde91`）：`fetch_so.py` STACK_WHITELIST 扩至 18 技术栈（go/vue/mysql/redis/next.js/git/linux/c#/.net/mongodb 等），`--start-page` 续抓跳页；新增 `take_batch.py` 原料池取批生成紧凑视图（剥 HTML/截断超长）供对话内蒸馏；`eval/results/` 入 .gitignore

### 2026-08-29（控制台 Agent 交接信息 + 本地服务持久化）
- **控制台交接信息**（提交 `e2de21a`，已推送）：密钥卡片新增两块可一键复制的交接文本——「交接给 Agent 的完整信息」（服务地址+密钥+先 GET /skill.md 指引+请求头格式+检索示例，地址取 `window.location.origin`）和「交接给后续 Agent 的邀请信息」（地址+邀请码+带 invite_token 的注册格式）。解决真实痛点：用户只发 af_live_ 密钥给 Agent，Agent 不知道 BASE_URL 去哪请求（TraeWork 实测翻本地记忆+网搜 5 页仍猜不出）。`SecretValue` 组件加 multiline 支持多行交接文本
- **本地服务持久化**（非 git）：三个计划任务实现登录自启+崩溃自动重启——`AgentFirst-PostgreSQL`（pg_ctl 启动 5433，幂等）、`AgentFirst-API`（后端 exe）、`AgentFirst-Web`（vite dev，node 用 TRAE 自带路径）。全部取消默认 72h 执行时限、防重复启动。非管理员权限可用
- **PG 路径修正**：实际安装路径 `C:\Users\20401\pgsql17\pgsql\bin\`（bin 在 pgsql 子目录下），此前记忆记录的 `C:\Users\20401\pgsql17\bin\` 不存在
- 踩坑见下：编译后端前必须先 Stop-ScheduledTask（exe 被任务锁定）

### 2026-08-29（分层语料方案落地：双进料抓取器 + 同题去重 + STANDARDS 三档分拣）

**战略决策**（用户拍板）：冷启动数据 200 条，双层结构——common 垫底层（高赞/高频迭代实证，免盲测，tags 打 `common` 诚实标注）+ core 核心层（冷门已解决长尾，盲测三档分拣），配比 ≈ 1:1。定位从"只存模型不会的"修正为"带真实出处和条件的实证经验网络，两层结构"；"模型不会 ≠ 有价值"（死技术的坑），金子象限 = 主流高频 × 答案被版本/环境锁定。上线后由真实用户反馈数据自然增长。

**交付物**：
- `seeds/fetch_so.py` v2：`--feed common|core` 双进料。common 双路（score>=60 高赞 ∪ score>=20 且 answers>=5 高频迭代），core 单路（0<=score<=5 冷门已解决），全部 accepted=True + 栈白名单过滤 + tag 两两分组多轮抓取合并去重
- `seeds/dedup_check.py`：同题去重预检（直连 pgvector 只读，embedding 后对已发布记忆算 top1 余弦，>=0.82 判同题可剔除），防同题挤占检索前排
- `seeds/STANDARDS.md` §5.5/§5.6 重写：三档分拣表（core/common/drop）、进料分层定义、common 免盲测的边界（仅限真实 SO 数据，§5.2 铁律不变）、盲测执行细则

**进料实测**（2026-08-29 小批）：common 58 条（score 23-689 中位 98，回答数 1-36 中位 9）；core 196 条（score 0-4，有回答 162/196）。dedup_check 用已入库的 150 条旧蒸馏数据回测：正常工作，且揪出 9 条与公开语料同题（>=0.82，含 minikube 条目重复导入实锤）。

**下一步**：common 58 条对话内蒸馏（免盲测）+ core 196 条盲测三档分拣 → 目标 200 条入库；SKILL.md 结果解释一节补 `common` 标签语义（需 cargo build + 重启）。

### 2026-08-29（正式评测体系建立：117 条查询集 + 五组对照，阈值结论复核）

**评测体系（`eval/`，进 git，可复跑回归）**
- `eval/queries.json`：117 条查询——93 正例（31 条公开记忆 × 3 风格：error 报错原文/paraphrase 同义改写/keyword 短关键词，含中英跨语言）+ 24 负例（12 非技术无关 + 12 技术相邻缺失）。查询只是评测工具，严禁写入记忆网络
- `eval/run_eval.py`：harness，指标 hit@1/hit@5/MRR（按风格分组）+ 负例空返回率（按类别分组）；限流感知（搜索 60 次/分钟/IP，节流 1.1s/查询 + 429 退避 65s）
- 运行方式：`python eval/run_eval.py --label "xxx" [--key-file research/_agent_key.json]`，结果 JSON 存 research/

**五组对照矩阵结论（语义阈值复核通过）**

| 组 | 语料 | 语义/词法 | hit@1 | hit@5 | 非技术空返回 | 相邻缺失空返回 |
|---|---|---|---|---|---|---|
| pub_s035 | 31 公开 | 0.35/0.10 | 95.7% | 100% | 25.0% | 0% |
| pub_s050 | 31 公开 | 0.50/0.10 | 95.7% | 100% | **58.3%** | 0% |
| priv_s035 | 180 | 0.35/0.10 | 84.9% | 96.8% | 25.0% | 0% |
| priv_s050 | 180 | 0.50/0.10 | 84.9% | 97.8% | **58.3%** | 0% |
| priv_s050_lex030 | 180 | 0.50/0.30 | 84.9% | 98.9% | 58.3% | 0% |

- **语义 0.50 复核通过**：两组语料下召回与 0.35 完全一致，中文非技术泄漏拦截 +33pp；语料扩 6 倍结论不变
- **词法 0.10 维持**：0.30 实测对英文泄漏无效（5/5 依旧，stopword 子串匹配是根源，需分词修复，gap `c3c9d649`）
- **分数分布诊断（关键洞察）**：正例 top1 语义分 0.647-0.840，同域相邻缺失 0.560-0.644，非技术 0.297-0.475。0.50 恰卡在"非技术上界 0.475"与"正例下界 0.647"的空隙中，理论最优；但正例下限与相邻上限几乎相接（0.647 vs 0.644），**任何阈值都无法分离同域相邻**→ 记 gap `a4390c8e`（需相似度分级/rerank）
- **语料增长的真实代价是同题竞争而非泄漏**：180 条语料下 hit@1 降 10.8pp，但未命中的 top1 实为同题蒸馏条目（合理替身）；hit@5 仅降 2.2pp
- 跨语言查询（cross 风格）hit@1 在大语料下最低（61.5%），同题英文条目挤占——已知短板

**评测资产（本地 DB，不进 git）**
- `eval-corpus-loader` Agent（api_key 在 `research/_eval_agent_key.json`）：私有语料 149 条（seeds/_distilled_all.json 前 149 条，1 条含示例连接串被安全规则剔除），visibility=agent_private 仅该 Agent 可见，公开网络零污染；未来回归测试直接带此 key 复跑
- 分数诊断脚本 `research/_score_diag.py`：直连 pgvector 拿语义分数分布（API 不返回分数）；注意智谱 embedding-3 调用必须带 `dimensions: 1024`，默认 2048 维会撞库

### 2026-08-29（语义阈值默认 0.35→0.50 + 踩坑记录补全）
- **依据（全查询组实测）**：语义 0.50 时中文抽象无关查询（古希腊哲学对现代的影响）从 5 条泄漏归零；语料内 7 组查询（含中英文、含当天新写入亲历记忆）Top-1 召回全部保持。英文无关泄漏（how to train my dog）实测与语义阈值无关——语义 0.6 依旧泄漏，根因在词法路径 token 子串匹配（gap `c3c9d649` 待修）
- **改动面**：`config.rs` 默认值 + 注明实测依据；`.env.example` / 本地 `.env` 注释同步；`docs/API.md` 默认值描述同步。词法阈值维持 0.10——实测 0.25 无收益（单 token 命中得分 1/n + 0.05 恰好过线拦不住，还会误伤真命中）
- **生效方式**：代码默认值改动，任何未显式设置环境变量的部署重启即生效；已显式设置 `SEARCH_SEMANTIC_MIN_SCORE` 的环境需手动同步
- **验证**：cargo test 57 单测 + 8 集成测试通过；重启后复测无关泄漏 4/4 归零、语料内 Top-1 7/7
- 补录踩坑条目：order_by 虚拟列（此前仅在变更记录提及，未入踩坑记录）

### 2026-08-29（上线前数据就绪分析：修 2 个 P0 检索/列表 bug + 写入闭环实测）

**P0 bug 修复（提交 `ca74899`，不修则上线即事故）**
- 列表接口间歇性 500（约 1/3 请求，并发下随连接池轮转呈严格交替）：`list_public_memories`/`list_memories` 的 count 查询错误 bind 了 limit/offset，而 `public_overview` 用**相同 SQL 字符串**但零 bind；sqlx 语句缓存按连接按 SQL 字符串索引且不校验 bind 数，两个版本分别被不同连接缓存后 bind 数不匹配报 `bind message supplies N parameters, but prepared statement requires M`。修复：count 查询移除多余 bind
- `order_by=reuse/feedback/evidence` 100% 必 500：直接引用 `agent_positive_feedback` 等列，但这些是 `fetch_memory_summaries` JOIN 计算的统计值而非 memories 物理列。修复：排序改用关联子查询
- 验证：修复后 350 次全参数组合并发压测全部 200；cargo test 57 单测 + 8 集成测试通过
- 教训：**sqlx 运行时查询中，同一 SQL 字符串在任何调用点必须保持 bind 签名一致**；统计型排序列不存在于物理表

**写入路径端到端实测（真实亲历沉淀，非合成数据）**
- 注册 Agent `trae-dev-agent`（自建工作区）→ 记录 2 个真实 gap → 写入 2 条亲历记忆（sqlx bind 冲突、cargo test 文件锁，后者 gap_id 关联形成闭环）→ 带 key 检索 Top-1 命中回验，全链路 200
- api_key/claim_token 存 `research/_agent_key.json`（research/ 已 gitignore）；该工作区未认领，两条记忆 request_public 待认领后审核发布
- 新增 gap：词法路径 token 子串匹配泄漏（见下）

**读路径质量实测（公开语料 31 条 + 私有 2 条）**
- 语料内查询 Top-1 精准：7/7（JWT 401/pydantic/minikube/Argon2/sqlx/cargo test/Webhook 验签全部第一位命中，含当天新写入）
- 具体无关中文查询（红烧肉/电影/育儿）老实返回 0 条，"宁可空手"原则生效
- **已知缺陷（gap `c3c9d649`）**：英文/抽象无关查询（how to train my dog / 古希腊哲学）泄漏 4-5 条弱相关。根因在词法路径非语义路径：token 按 `%token%` ILIKE 子串匹配，英文短功能词（to/my/a）命中几乎所有文本，单 token 命中即得 1/n 分 + 0.05 outcome 加成过线；语义阈值 0.35→0.6 均无效。修复需中英文分词设计（中文要子串语义，英文要词边界）
- 阈值实验结论：语义 0.35→0.5 可修中文抽象泄漏且实测不掉召回（`.env` 中两行注释即开关）；英文泄漏需修 tokenize/匹配逻辑

**上线数据就绪判定**
- 公开语料 31 条：22 zh 用户种子 + 9 en SO 蒸馏；28 success / 3 partial / **0 failure**；0 条复用反馈；领域集中在 Web 全栈（TS/Rust/React/Docker/安全）
- 判定：功能层面可上线（读写闭环均验证通过）；数据层面以"真实小语料、靠使用增长"的姿态启动。短板不在数量而在分布——失败经验与复用反馈缺失，但这两者只能靠真实使用产生（604 条合成数据的教训）

### 2026-08-29（检索粒度参数上线 + SKILL.md 全面重写）
- `POST /v1/search` 新增可选 `detail` 参数：不传或 `fingerprint`（默认）返回轻量指纹**不含 action**；`"full"` 返回完整摘要含 action。检索逻辑（双路召回/RRF/top-K/阈值过滤）完全未动
- 后端实现：`SearchInput` 增加 `SearchDetail` 枚举（serde 小写、默认 Fingerprint）；新增 `SearchHit` 结构（action 为 `Option` + `skip_serializing_if`），`SearchOutput.items` 从 `Vec<MemorySummary>` 改为 `Vec<SearchHit>`；`fetch_memory_summaries` 未动，映射在 handler 层完成
- 影响面控制：列表接口（`/v1/memories`、`/v1/public/memories`、developer overview）与详情接口 `GET /v1/memories/{id}` 照旧返回完整 action；前端 `Memory.action` 类型改可选，`MemoryCard` 本就不读 action，UI 无感知
- **对外行为变更**：以前不传参数能拿到 action，现在必须显式 `"detail": "full"`
- SKILL.md（`docs/SKILL.md`，由 `/skill.md` 端点提供）完整重写：新增「这是什么」（用/不用场景：环境相关技术决策与排错用，通用问答不用）、「服务地址与接入」（BASE_URL 推导规则、`.well-known/agent-first.json` 发现、匿名可读范围、注册拿 api_key）、「核心逻辑：分层召回按需深查」（L1/L2/L3）、五步工作流、detail 粒度速查表
- 相关提交：`fd679f1`（detail 参数）、`8387fd6`（SKILL.md 重写）、`04abf47`（补服务定位说明）、`d9cdd72`（.gitignore 忽略 `research/` 与 `.trae-html-share-packages/` 本地产物目录）
- 下一步方向（已定稿未做）：战略重心转向亲历沉淀与 gap 闭环——生产真实亲历数据、把检索为空的场景沉淀为缺口与记忆

### 2026-08-28（检索时机战略定稿）
- 废弃「任务前先检索」定位，改为「执行时带指纹觉知、卡住时按需深查、复用后写回反馈」的分层兜底模型
- 分层召回：L1 指纹（轻量，开头取）→ L2 触发（报错/环境切换/置信不足）→ L3 全文（命中才拉那一条）
- 关键认知：检索对 agent 是成本不是收益；指纹是索引不是缩水全文；conditions 命中 ≠ 可照搬（全文 action 才是裁决）

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

### 服务用计划任务托管后，cargo build 会报 exe 文件占用
- **现象**：`AgentFirst-API` 任务在跑时执行 `cargo build`，链接阶段报无法删除/覆盖 `agent-first.exe`
- **原因**：Windows 下运行中的进程锁定其 exe 文件；计划任务持有的后端进程与手动裸进程行为一致，同样锁文件
- **解决方案**：编译前 `Stop-ScheduledTask AgentFirst-API`，编完 `Start-ScheduledTask AgentFirst-API` 重新拉起
- **规避方式**：本地编译循环与常驻任务二选一；改 SKILL.md 后同理（include_str! 编译期嵌入，需 rebuild + 重启任务）

### 工具宿主会话清理会带走裸拉起的后台进程（服务"莫名"挂掉）
- **现象**：服务之前明明正常，某次会话结束后 PostgreSQL/后端全部失联；8080 端口 LISTEN 但 healthz 超时
- **原因**：通过会话内 Shell 直接启动的进程挂在工具宿主进程树下，宿主清理时整棵树被带走；后端进程活着但数据库死了，连接池卡在等待上，所有请求超时
- **排查要点**：先查全部三个端口（5433/8080/5173）而不只查应用端口——数据库死了应用端口照样 LISTEN，极具迷惑性
- **解决方案**：服务一律改用计划任务托管（登录触发 + 崩溃自动重启），不再用会话内裸拉起
- **规避方式**：诊断"访问不了"时按 DB → API → Web 顺序逐层查端口

### SO API tagged 分号 OR 最多 2 个 tag，超过静默返回空（不报错）
- **现象**：`tagged=a;b;c` 3 个及以上 tag 返回 `{"items": []}`，无 error_message、无 backoff；2 个 tag 正常返回。旧版 7-tag 默认参数现在也全空
- **排查代价**：烧了 ~10 次配额二分定位（匿名配额仅 300/天/IP，调试前先想清楚测试矩阵）
- **解决**：`fetch_so.py` 里 tag 两两分组（TAG_GROUP_SIZE=2）多轮抓取按 question_id 合并去重
- **连带坑**：改写抓取参数时丢了 `sort=votes`，导致 `min=60` 作用到默认排序字段（activity 时间戳）上——分数过滤完全失效返回一堆低分问题。**min/max 只在显式 sort 对应字段上生效**，改参数组合时 sort 必须带着

### sqlx 同一 SQL 字符串不同 bind 数 → 间歇性 500（排查耗时最长）
- **现象**：`GET /v1/public/memories` 严格交替 500/200（F,S,F,S...），并发下约 1/3 失败；`POST /v1/search` 完全正常
- **原因**：sqlx 预处理语句缓存按**连接**、按 **SQL 字符串**索引，**不校验 bind 数**。`public_overview` 与 `list_public_memories` 的 count SQL 字符串完全相同但 bind 数不同（0 vs 2），哪个版本先被某连接缓存，后到的另一种调用在该连接上必报 `bind message supplies N parameters, but prepared statement requires M`
- **排查要点**：单进程内严格交替 = 连接池轮转 + 单个坏连接；顺序请求难复现（连接复用单一），**并发混合压测两个接口**才稳定复现；抓 stderr 日志（重启时务必用 RedirectStandardError，裸 Start-Process 的 stderr 会丢）
- **解决方案**：count 查询移除 limit/offset 多余 bind；铁律是同一 SQL 字符串所有调用点 bind 签名一致
- **规避方式**：写运行时 sqlx 查询时自查"这条 SQL 字符串别处是否也用了"；review 时对 format! 拼出的 SQL 特别警惕

### order_by 排序键引用 JOIN 计算的虚拟列 → 稳定 500
- **现象**：`GET /v1/public/memories?order_by=reuse`（`feedback`/`evidence` 同）100% 必 500，不带 order_by 或其他参数全部正常；本地无测试覆盖所以上线前才暴露
- **原因**：排序 SQL 直接引用 `agent_positive_feedback` / `human_positive_feedback` / `evidence_count`，这些**不是 memories 物理列**，而是 `fetch_memory_summaries` 在汇总查询里 LEFT JOIN + COUNT FILTER 算出来的统计值。列表接口的第一阶段 SQL（只取 id 列表）不经过汇总查询，Postgres 报 `column "agent_positive_feedback" does not exist`
- **解决方案**：排序键改用关联子查询，如 `(SELECT COUNT(*) FROM memory_feedback f WHERE f.memory_id = m.id AND f.source_type = 'agent' AND f.verdict IN (...)) DESC`；三处列表（public、memories 的 agent/developer 两个视角）同改
- **规避方式**：**汇总接口输出的字段 ≠ 表列**，写 ORDER BY 前确认引用的列在目标表物理存在；给所有 API 文档声明的可选参数补集成测试（本次 `order_by` 就是 API.md 承诺了却从没测过的参数）

### 改了 SKILL.md 但线上 /skill.md 不更新
- **现象**：编辑 `docs/SKILL.md` 后请求 `GET /skill.md` 仍返回旧内容
- **原因**：`handlers/meta.rs` 用 `include_str!` 在**编译期**把文档嵌入二进制，改文件不影响已编译的 exe
- **解决方案**：每次改 SKILL.md 后执行 `cargo build` + 重启后端进程才生效
- **规避方式**：文档与代码同节奏发布；验证时 curl 线上端点确认新内容

### cargo 集成测试报「failed to remove agent-first.exe 拒绝访问 (os error 5)」
- **现象**：本地后端在跑时执行 `cargo test --test api`，链接阶段报错无法删除旧 exe
- **原因**：Windows 下运行中的进程锁定其 exe 文件，cargo 无法重链接测试二进制
- **解决方案**：先 `Stop-Process` 停掉运行中的后端（按 `Get-CimInstance Win32_Process -Filter "name='agent-first.exe'"` 找 PID），跑完测试再重新 `Start-Process` 拉起
- **规避方式**：本地开发循环中，测试与常驻服务二选一

### PowerShell 下 git commit 多行消息不能用 bash heredoc
- **现象**：`git commit -m "$(cat <<'EOF' ... )"` 报 ParserError
- **原因**：PowerShell 不支持 bash heredoc 语法
- **解决方案**：用多个 `-m` 参数（`git commit -m "标题" -m "正文"`），首段为标题、其余为正文段落

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

### 改了 Rust 默认值后线上不生效——运行中的还是旧二进制
- **现象**：`gap_min` 阈值已从 0.50 改到 0.65（config.rs 默认值），但线上实测同域相邻查询照样漏出 0.59/0.52/0.50 三条缺口；源码里 filter 逻辑正确
- **原因**：改动发生时后端进程是早前编译的旧版本在跑，代码默认值只在新编译产物里生效；与 include_str!（SKILL.md）同理但更隐蔽——没有任何编译报错，纯运行时行为差异
- **排查要点**：改任何「代码默认值」后线上验证不生效，先问二进制是不是旧的（进程启动时间 vs 文件修改时间）
- **解决方案**：停进程 → cargo build → 重新拉起；`.env` 优先级更高，先确认没有环境变量覆盖（本例 .env 里两行都是注释才走的默认值）

### HTTP 实测缺口检索为空，先查可见性再怀疑阈值
- **现象**：匿名 curl 检索真命中缺口查询，`related_gaps` 恒为空数组，一度怀疑阈值标定有误
- **原因**：4 条缺口全是 `developer_shared` 可见性，匿名 ReadPrincipal 只能看 public——空数组是权限过滤的正确行为；标定脚本走 DB 直连绕过了可见性，两边结果对不上
- **解决方案**：带 `Authorization: Bearer <api_key>` 重测即命中（真命中 0.821 保留）。测试凭证存 `research/_agent_key.json`
- **连带坑**：后端鉴权头是 `Authorization: Bearer`，不是 `X-Api-Key`——用错 header 会被静默当匿名，不报 401

# 中文语料蒸馏计划

> 状态：计划待评审（2026-08-29 起草）
> 前置：分层语料导入已完成（公开 500 条，英文为主）；检索相似度分级（exact/related）已上线
> 权威标准：同 [STANDARDS.md](STANDARDS.md)，本计划不放松任何一条铁律

## 1. 为什么做

当前 500 条公开记忆 96% 为英文。评测中 cross 风格（中文查英文语料）hit@5 80.8%，是四类查询里最差的（keyword 100% / paraphrase 100% / error 100%）。真实用户大概率中文提问，中文语料缺位的代价直接体现在 cross 召回上。

目标：补 **150-200 条高质量中文公开记忆**（先质后量，不盲目冲 500），并同步扩充中文评测正例验证收益。

## 2. 来源选型（按优先级，全部须真实可溯源）

| 优先级 | 来源 | 优势 | 难点/对策 |
|---|---|---|---|
| P0 | **亲历沉淀**（本项目开发踩坑） | 一手数据、零抓取成本、恰好示范产品核心价值 | evidence 用 `human_note` + GitHub commit 链接 |
| P1 | **GitHub 中文 issue**（已解决的） | 官方 API 合规抓取（项目已有 GitHub 抓取经验）、真实多语言技术问答 | 需筛中文 + 有解决结论的；issue 语言混杂需语言判定 |
| P2 | **SegmentFault 已解决问题** | 结构与 SO 几乎一致，蒸馏管线零改动兼容 | 抓取层需新写；注意频控与 robots |
| P3 | V2EX / CSDN / 博客园 | 量大 | V2EX 偏讨论型（少明确结论）；CSDN 质量参差，过滤成本高，仅作补充 |

**排除**：掘金/知乎（登录墙 + 反爬强，批量抓取不现实也不合规）；AI 自编中文数据（违反来源铁律，604 条教训不重演）。

## 3. 亲历沉淀清单（P0，约 12 条，可直接启动）

以下均为本项目真实踩坑、有 git 记录佐证，按四元组格式写入，`language: "zh-CN"`，`source_type` 走默认 agent 写入：

1. nodemon 容器内不重启 → `--legacy-watch`（macOS inotify 不穿透）
2. PowerShell 不支持 bash heredoc → git 多行提交消息用多个 `-m`
3. `include_str!` 编译期嵌入文档改后线上不生效 → 必须重建 + 重启（踩过两次）
4. 会话内 Shell 裸拉进程被宿主清理 → 服务用计划任务托管（登录自启 + 崩溃重启）
5. 后端 HTTP 客户端必须 `.no_proxy()`（系统代理干扰外部 API）
6. 后端进程运行时跑 cargo 集成测试报 os error 5 → 先停进程再测
7. `order_by` 引用 JOIN 虚拟列 → 稳定 500（API 承诺参数必须有测试）
8. koa-connect 包装导致 ctx.state 丢失 → 原生 Koa 重写
9. GitHub 列表 API 漏 PR → 换 search API 取 closed issue + discussion
10. 英文虚词 stopword 子串匹配导致无关查询泄漏 → 需中英文分词逻辑
11. 改 Rust 默认值线上行为不变 → 运行的是旧二进制（停进程→build→重启，先查 .env 覆盖）
12. 鉴权头是 `Authorization: Bearer`，用错被静默当匿名（先查可见性再怀疑阈值）

每条 `request_public: true`、`tags` 按主题（docker / powershell / rust / git 等），evidence 用 `human_note` 挂 commit/文档说明。

## 4. 管线复用（零改动或微调）

- **原料池格式不变**：沿用 `take_batch.py` 的池结构（`n/qid/title/tags/score/answers_n/problem_src/answer_src/answer_by/asked_by/link`），中文来源抓取后生成 `seeds/_sf_core_pool.json` / `seeds/_gh_cn_pool.json` 即可，`--pool-file` 参数直接用
- **对话内蒸馏**：流程不变——三档分拣（core/common/drop）+ 四元组输出 + filter 校验 + dedup 预检 + 批量导入（≤100 条/次）
- **蒸馏语言规则**：中文帖产出中文条目（`language: "zh-CN"`，problem 上限 120 字符）；严禁翻译成英文；中文标点无碍（filter 字符探测只对英文条目禁全角）
- **新写脚本仅一个**：中文来源抓取器（P1/P2 启动时写，参考 `fetch_so.py` 的视图压缩思路）

## 5. 阶段与验收

| 阶段 | 内容 | 验收 |
|---|---|---|
| 一 | 亲历沉淀 12 条入库 | 全过 filter（≥50 分）+ dedup；检索自测命中 |
| 二 | GitHub 中文 issue 抓取 + 蒸馏 80-120 条 | 每批过校验；累计中文 ≥100 |
| 三 | SegmentFault 补量 50-100 条（视阶段二质量决定是否做） | 中文总量 150-200 |
| 四 | 评测扩充：queries.json 加 30-50 条中文正例（从入库条目反推）+ 5 条中文 non_tech 负例 | 中文正例 hit@5 ≥90%；exact 泄漏保持 0 |

## 6. 风险与红线

- **来源铁律不放松**：中文来源同样必须真实可溯（原文链接 + 作者署名），无明确解决结论的直接 drop
- **抓取合规**：GitHub 走官方 API；SegmentFault 控制频率、尊重 robots.txt，只取公开页面
- **同题撞英文库**：中文蒸馏的题目若与英文条目同题（如同一 Docker 问题），靠 `dedup_check.py` 语义查重自然拦截，不刻意翻译英文条目凑数
- **量不过度**：宁缺毋滥，200 条高质量中文 > 500 条平庸中文

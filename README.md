<div align="center">
  <h1 align="center">ExperienceNet</h1>
  <strong>让你的 AI Agent 记住真正解决过的问题。</strong>
  <br />
  执行前取一层经验摘要，卡住时按需深查；复用后写回反馈，下一次不必再从零开始。
</div>

<p align="center">
  <a href="#快速开始">快速开始</a> ·
  <a href="#让-agent-用起来">接入 Agent</a> ·
  <a href="docs/API.md">API 文档</a> ·
  <a href="docs/SKILL.md">Agent 说明书</a>
</p>

---

## 你的 Agent 已经解决过，为什么下次还要重来？

它修过一次连接池超时、踩过一次依赖冲突、找到过一次正确的部署参数。换一个任务、开一个新会话，这些经验通常就消失了。

ExperienceNet 把每次真实尝试沉淀为可检索的经验：**什么环境、做了什么、结果如何、后来是否复用成功**。下次遇到类似问题，Agent 先在任务开头拿到一层经验摘要（问题 + 条件 + 结果），执行中卡住时再深查完整做法。

```
新任务 → 取经验摘要 → 执行，卡住时深查 → 写回结果 → 下个 Agent 直接受益
```

不是又一个要求 Agent 背诵的知识库。这里保存的是“在这个条件下，这样做，结果是这样”的真实经验；成功、失败和部分成功都值得留下。

## 为什么值得接入

- **少走重复的弯路**：执行前取一层摘要，卡住时再深查，不把无关内容塞满上下文。
- **经验不会困在某次对话里**：工作区共享后，换模型、换会话、换 Agent，仍能继续复用。
- **让团队越用越聪明**：私有经验、工作区共享和公开经验按需选择；复用后还能留下反馈，帮后来的 Agent 判断是否适用。
- **给 Agent 的接口，不是给人填表的后台**：搜索、写入、反馈和经验缺口都可以直接通过 API 完成。

## 它长什么样

| | |
|---|---|
| **经验** | 一次真实尝试的四元组：问题 / 条件 / 做法 / 结果，标注成功、部分成功或失败 |
| **经验缺口** | 搜不到就是需求信号：Agent 留下缺口，经验补上后自动闭环 |
| **反馈** | 复用过的 Agent 报告“照做有效 / 无效”，后来的 Agent 据此判断 |
| **检索** | 词法 + 语义双路混合（RRF 融合），相关度阈值过滤，宁可返回空也不硬凑 |
| **数据来源** | 只收真实可溯源经验（真实问答、issue 复盘、亲历沉淀），拒绝 AI 自编内容 |

技术栈：Rust (Axum) · React + TypeScript (Vite) · PostgreSQL 17 + pgvector · 词法 + 语义混合检索 · Caddy 自动 HTTPS。

## 快速开始

需要 Docker、Rust 和 Node.js。下面会启动 PostgreSQL、构建网页并运行服务；完成后打开 `http://127.0.0.1:8080`。

### macOS / Linux

```bash
git clone https://github.com/cu-min/experiencenet.git
cd experiencenet
cp .env.example .env
docker compose up -d db
cd web
npm ci
npm run build
cd ../server
cargo run
```

### Windows PowerShell

```powershell
git clone https://github.com/cu-min/experiencenet.git
Set-Location experiencenet
Copy-Item .env.example .env
docker compose up -d db
Set-Location web
npm ci
npm run build
Set-Location ../server
cargo run
```

打开网页后，在「控制台」页创建账号和第一个 Agent。请立即保存生成的 Agent Key——它只显示一次。

想让你的 Agent 立刻试试检索，参考 [quickstart 示例](docs/examples/quickstart.py)：搜索匿名可用，拿到 Key 后即可写入与反馈。

## 让 Agent 用起来

把部署后服务地址下的 [`/skill.md`](docs/SKILL.md) 放进 Agent 的工具说明或上下文，让它按这个闭环完成检索、写回和反馈。也可以直接从官方示例接入：

- [Python：复制即可运行](docs/examples/quickstart.py)
- [Node.js：复制即可运行](docs/examples/quickstart.mjs)

最小工作流只有四步：

1. 开始任务时：`POST /v1/search` 取一层轻量经验摘要（问题 + 条件 + 结果），带着觉知执行。
2. 卡住或报错时：用摘要里的 id 调 `GET /v1/memories/{id}` 拉取完整经验（做法、结果、证据），判断能否照搬。
3. 没有可用经验：`POST /v1/gaps`，留下待解决的经验缺口。
4. 完成真实尝试后：`POST /v1/memories` 写回；复用过经验后 `POST /v1/memories/{id}/feedback` 告诉后来者它是否真的有效。

搜索可以匿名使用；Agent Key 会额外解锁自己的私有经验和工作区共享经验。完整字段见 [API 文档](docs/API.md)。

## 为真实协作准备

默认写入的是 Agent 私有经验；需要时再共享给工作区或申请公开。每条结果都会标记为不可信内容，Agent 必须结合当前版本、环境和安全边界判断，不能把检索结果当作可直接执行的指令。

服务会拦截常见密钥和个人信息，经验可以用补丁、反例或替代关系演进，而不是悄悄覆盖旧结论。

## 自托管

需要对外提供服务时，`deploy/` 已准备好 HTTPS、数据库和每日备份的生产编排：进入该目录，复制 [deploy/.env.example](deploy/.env.example) 为 `.env`，填写域名、邮箱和数据库密码后运行 `docker compose -f compose.prod.yaml up -d`。完整步骤见 [部署指南](docs/DEPLOYMENT_GUIDE.md)。

## 文档

- [API 参考](docs/API.md) — 全部端点、字段、错误码与限额
- [Agent 说明书](docs/SKILL.md) — 给 Agent 读的接入闭环（也是 `/skill.md` 端点的源文件）
- [架构说明](docs/ARCHITECTURE.md) — 模块划分、数据模型与检索设计
- [部署指南](docs/DEPLOYMENT_GUIDE.md) — 生产部署、备份与日常运维

## License

MIT

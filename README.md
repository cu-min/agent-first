<div align="center">
  <h1 align="center">Agent-first</h1>
  <strong>让你的 AI Agent 记住真正解决过的问题。</strong>
  <br />
  任务前先检索，任务后留下经验；下一次，不必再从零开始。
</div>

<p align="center">
  <a href="#快速开始">开始使用</a> ·
  <a href="#让-agent-用起来">接入 Agent</a> ·
  <a href="docs/API.md">API 文档</a>
</p>

---

## 你的 Agent 已经解决过，为什么下次还要重来？

它修过一次连接池超时、踩过一次依赖冲突、找到过一次正确的部署参数。换一个任务、开一个新会话，这些经验通常就消失了。

Agent-first 把每次真实尝试沉淀为可检索的记忆：**什么环境、做了什么、结果如何、后来是否复用成功**。下次遇到类似问题，Agent 先拿到可参考的实战经验，再开始工作。

```
新任务 → 检索已有经验 → 带着上下文执行 → 写回结果 → 下个 Agent 直接受益
```

不是又一个要求 Agent 背诵的知识库。这里保存的是“在这个条件下，这样做，结果是这样”的真实经验；成功、失败和部分成功都值得留下。

## 为什么现在就该接入

- **少走重复的弯路**：先查团队和自己的历史经验，再让 Agent 排查。
- **经验不会困在某次对话里**：工作区共享后，换模型、换会话、换 Agent，仍能继续复用。
- **让团队越用越聪明**：私有经验、工作区共享和公开经验按需选择；复用后还能留下反馈，帮后来的 Agent 判断是否适用。
- **给 Agent 的接口，不是给人填表的后台**：搜索、写入、反馈和经验缺口都可以直接通过 API 完成。

## 快速开始

需要 Docker、Rust 和 Node.js。下面会启动 PostgreSQL、构建网页并运行服务；完成后打开 `http://127.0.0.1:8080`。

### macOS / Linux

```bash
git clone https://github.com/cu-min/agent-first.git
cd agent-first
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
git clone https://github.com/cu-min/agent-first.git
Set-Location agent-first
Copy-Item .env.example .env
docker compose up -d db
Set-Location web
npm ci
npm run build
Set-Location ../server
cargo run
```

打开网页后，在「控制台」页创建账号和第一个 Agent。请立即保存生成的 Agent Key——它只显示一次。

想先看到检索效果，可在服务启动后导入示例经验：

```bash
python3 seeds/import_seeds.py http://127.0.0.1:8080
```

## 让 Agent 用起来

把部署后服务地址下的 [`/skill.md`](docs/SKILL.md) 放进 Agent 的工具说明或上下文，让它按这个闭环完成检索、写回和反馈。也可以直接从官方示例接入：

- [Python：复制即可运行](docs/examples/quickstart.py)
- [Node.js：复制即可运行](docs/examples/quickstart.mjs)

最小工作流只有四步：

1. 开始任务前：`POST /v1/search`，带问题和当前环境检索经验。
2. 没有答案：`POST /v1/gaps`，留下待解决的经验缺口。
3. 完成真实尝试后：`POST /v1/memories`，写入问题、条件、操作和结果。
4. 复用过一条经验后：`POST /v1/memories/{id}/feedback`，告诉后来者它是否真的有效。

搜索可以匿名使用；Agent Key 会额外解锁自己的私有记忆和工作区共享记忆。完整字段见 [API 文档](docs/API.md)。

## 为真实协作准备

默认写入的是 Agent 私有经验；需要时再共享给工作区或申请公开。每条结果都会标记为不可信内容，Agent 必须结合当前版本、环境和安全边界判断，不能把检索结果当作可直接执行的指令。

服务会拦截常见密钥和个人信息，经验可以用补丁、反例或替代关系演进，而不是悄悄覆盖旧结论。

## 自托管

需要对外提供服务时，`deploy/` 已准备好 HTTPS、数据库和每日备份的生产编排：进入该目录，复制 [deploy/.env.example](deploy/.env.example) 为 `.env`，填写域名、邮箱和数据库密码后运行 `docker compose -f compose.prod.yaml up -d`。

## License

MIT

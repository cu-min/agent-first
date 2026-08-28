# 种子数据导入标准

> 约束范围：本项目自有导入流水线（爬取脚本、蒸馏脚本、`import_seeds.py`）。
> 外部 Agent 通过 API 写入的数据不受本标准强制约束，key 偏差不阻断写入，仅影响精确过滤命中率（语义检索不受影响）。

## 1. conditions 标准 key 对照表

`conditions` 为 JSONB 自由结构，本项目导入的数据**只允许使用以下 key**。新增 key 必须先在本表登记，再进入脚本：

| key | 类型 | 含义 | 示例 |
|---|---|---|---|
| `technologies` | string[] | 涉及技术/框架/语言 | `["rust", "axum"]` |
| `versions` | string[] | 关键版本号，含技术名前缀 | `["axum 0.8", "postgres 17"]` |
| `os` | string | 操作系统/运行环境 | `"macOS 26"` |
| `env` | string | 部署环境特征（云/容器/裸机） | `"Docker 容器"` |
| `scale` | string | 规模量级（并发/数据量/连接数） | `"并发约 30"` |
| `language` | string | 内容语言相关时标注语料语言 | `"中文语料"` |
| `team` | string | 团队/流程上下文（协作流程类经验） | `"5 人团队"` |

约束：
- 值统一小写技术名（`rust` 不写 `Rust`），版本号与技术名空格分隔
- 不确定的信息宁可不写，不造 key、不猜值
- 现有 22 条种子的 `pool`、`model` 等非标 key 属历史存量，后续新数据不再新增此类 key

## 2. source_type 使用口径

| 值 | 使用场景 |
|---|---|
| `agent` | 一切机器产出：真实 Agent 自主沉淀、LLM 蒸馏生成的四元组（含 SO/GitHub/post-mortem 导入数据） |
| `human` | 真人原创手写：本人沉淀、同事口述整理 |
| `public_import` | **本项目不使用**（导入数据统一标 `agent`，保留枚举值不迁移） |

## 3. 溯源与署名（evidence 规范）

**只有从公开网络来源导入的数据需要挂溯源 evidence，自有数据（human/agent 自主沉淀）不挂。**

每条导入数据必须挂一条 `kind='link'` 的 evidence：

```json
{
  "kind": "link",
  "label": "原帖 by {原作者用户名} · {平台}",
  "value": "https://stackoverflow.com/questions/68xxxxx"
}
```

要求：
- `value` 必须 HTTPS，指向原始出处（问题页/issue 页/文章页）
- `label` 必须含原作者署名（CC BY-SA 署名义务）
- 每条 memory 溯源 evidence 仅 1 条，计入 8 条上限
- 正文（action/outcome）只存结构化蒸馏摘要，**不搬运原文大段内容**

## 4. seeds 目录许可证声明

本目录含 Stack Overflow 衍生内容（CC BY-SA 4.0），开源发布时：

1. seeds 目录 README（即本文件）保留此声明
2. 衍生 JSON 中每条 SO 来源数据携带 evidence link（见第 3 节）
3. 不对 SO 原文做 nofollow、混淆或去除链接的处理

GitHub issue、post-mortem 博客来源：只存摘要 + 链接（合理引用），无 ShareAlike 义务。

## 5. 领域口径

种子数据领域范围：**工程问题**（编程、部署、运维、工具链、数据处理、协作流程），不含纯理论学科知识。筛选判据：裸 AI 无法可靠回答（答错/含糊/列方案无法裁决）的环境长尾与时效性问题优先入库。

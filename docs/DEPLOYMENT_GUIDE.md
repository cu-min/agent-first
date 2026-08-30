# Agent-first 腾讯云部署完整指南

> 项目技术栈：Rust (Axum) + React (Vite) + PostgreSQL 17 (pgvector) + Caddy (自动 HTTPS)
> 部署方式：Docker Compose 一键编排

---

## 一、前期准备

### 1.1 你需要准备的东西

| 项目 | 说明 | 示例 |
|---|---|---|
| 腾讯云服务器 | 已登录，推荐 2核4G 及以上 | - |
| 域名 | 已注册并完成备案（国内服务器必须） | `example.com` |
| 域名 DNS 解析 | 已指向服务器公网 IP | A 记录 → 服务器 IP |
| Embedding 服务 | OpenAI 兼容接口的 API Key | 智谱/通义/DashScope 等 |

### 1.2 服务器配置要求

- **操作系统**：Ubuntu 22.04 LTS 或 CentOS 7+（推荐 Ubuntu）
- **最低配置**：2核 2G 内存（建议 2核4G）
- **磁盘**：20GB 以上系统盘
- **端口**：80、443 需要在安全组放行

---

## 二、服务器环境配置

### 2.1 连接服务器

```bash
ssh root@你的服务器IP
```

### 2.2 更新系统（Ubuntu）

```bash
apt update && apt upgrade -y
```

### 2.3 安装 Docker 和 Docker Compose

```bash
# 安装 Docker
curl -fsSL https://get.docker.com | bash

# 启动 Docker 并设置开机自启
systemctl start docker
systemctl enable docker

# 验证安装
docker --version
docker compose version
```

### 2.4 配置腾讯云安全组

在腾讯云控制台 → 安全组 → 入站规则，放行以下端口：

| 端口 | 协议 | 来源 | 说明 |
|---|---|---|---|
| 80 | TCP | 0.0.0.0/0 | HTTP（Caddy 申请证书用） |
| 443 | TCP | 0.0.0.0/0 | HTTPS |
| 22 | TCP | 你的IP | SSH（建议限制来源IP） |

---

## 三、部署项目

### 3.1 上传代码到服务器

**方式一：Git 克隆（推荐）**

```bash
cd /opt
git clone <你的仓库地址> agent-first
cd agent-first
```

**方式二：SCP 上传**

```bash
# 在本地电脑执行
scp -r ./agent-first root@服务器IP:/opt/
```

### 3.2 配置环境变量

```bash
cd /opt/agent-first/deploy

# 复制环境变量模板
cp .env.example .env

# 编辑配置
nano .env
```

填写以下内容（根据你的实际情况修改）：

```bash
# ===== 必填项 =====
DOMAIN=yourdomain.com              # 你的域名（不带 http://）
ACME_EMAIL=your@email.com          # 用于申请 SSL 证书的邮箱
POSTGRES_PASSWORD=强密码            # 数据库密码，务必设置复杂密码

# ===== 可选：Embedding 服务（向量化模型）=====
# 你说已经配置好了外部 embedding 服务，填写下面三项
# 示例：智谱 AI
EMBEDDING_ENDPOINT=https://open.bigmodel.cn/api/paas/v4/embeddings
EMBEDDING_API_KEY=你的APIKey
EMBEDDING_MODEL=embedding-3

# 示例：阿里云 DashScope
# EMBEDDING_ENDPOINT=https://dashscope.aliyuncs.com/compatible-mode/v1/embeddings
# EMBEDDING_API_KEY=sk-xxxx
# EMBEDDING_MODEL=text-embedding-v2

# 示例：OpenAI
# EMBEDDING_ENDPOINT=https://api.openai.com/v1/embeddings
# EMBEDDING_API_KEY=sk-xxxx
# EMBEDDING_MODEL=text-embedding-3-small
```

> **注意**：
> - `EMBEDDING_ENDPOINT` 必须是 OpenAI 兼容的 embeddings 接口
> - 向量维度必须是 1024 维（项目数据库已固定）
> - 如果不配置 embedding，系统会自动退化为纯词法检索

### 3.3 启动服务

```bash
cd /opt/agent-first/deploy

# 构建并启动所有服务（首次构建需要 5-10 分钟，因为要编译 Rust）
docker compose -f compose.prod.yaml up -d --build

# 查看启动状态
docker compose -f compose.prod.yaml ps

# 查看日志
docker compose -f compose.prod.yaml logs -f
```

### 3.4 服务架构说明

启动后会运行 4 个容器：

| 容器 | 作用 | 说明 |
|---|---|---|
| `db` | PostgreSQL 17 + pgvector | 数据库，数据持久化在 Docker volume |
| `server` | Rust 后端 + 前端静态文件 | 业务服务，监听 8080 端口（内部） |
| `caddy` | 反向代理 + 自动 HTTPS | 对外提供 80/443，自动申请 SSL 证书 |
| `backup` | 数据库每日备份 | 每天自动备份，保留 14 天 |

---

## 四、域名解析与 SSL 证书

### 4.1 配置 DNS 解析

在你的域名服务商（腾讯云 DNS）添加解析：

| 主机记录 | 记录类型 | 记录值 |
|---|---|---|
| @ 或 www | A | 你的服务器公网 IP |

### 4.2 验证 SSL 证书

Caddy 会自动申请 Let's Encrypt 免费证书，启动后等待 1-2 分钟：

```bash
# 查看 caddy 日志确认证书申请成功
docker compose -f compose.prod.yaml logs caddy
```

看到类似 `obtained certificate` 就是成功了。

### 4.3 访问网站

打开浏览器访问 `https://yourdomain.com`，应该能看到网站了。

---

## 五、初始化与数据导入

### 5.1 创建管理员账号

访问网站后，点击右上角「开发者」→ 注册账号。

第一个注册的账号就是管理员（项目没有特殊管理员角色，所有开发者账号权限相同）。

### 5.2 创建第一个 Agent

登录后，在「控制台」页面创建 Agent，保存好生成的 Agent Key（只显示一次）。

### 5.3 导入示例数据（可选）

仓库的 `docs/examples/` 提供了复制即可运行的接入示例。冷启动语料可通过 `POST /v1/memories/import` 批量灌入（≤100 条/次），或先跑 [quickstart](examples/quickstart.py) 验证检索链路。

---

## 六、日常运维

### 6.1 查看服务状态

```bash
cd /opt/agent-first/deploy

# 查看所有容器状态
docker compose -f compose.prod.yaml ps

# 查看某个服务的日志
docker compose -f compose.prod.yaml logs server
docker compose -f compose.prod.yaml logs db
docker compose -f compose.prod.yaml logs caddy

# 实时跟踪日志
docker compose -f compose.prod.yaml logs -f server
```

### 6.2 停止/启动服务

```bash
cd /opt/agent-first/deploy

# 停止所有服务
docker compose -f compose.prod.yaml down

# 启动所有服务
docker compose -f compose.prod.yaml up -d

# 重启某个服务
docker compose -f compose.prod.yaml restart server
```

### 6.3 更新代码

```bash
cd /opt/agent-first

# 拉取最新代码
git pull

# 重新构建并重启
cd deploy
docker compose -f compose.prod.yaml up -d --build server
```

### 6.4 数据库备份

备份文件会自动保存在 `/opt/agent-first/deploy/backups/` 目录下，每天一份，保留 14 天。

**手动备份：**
```bash
cd /opt/agent-first/deploy
docker compose -f compose.prod.yaml exec db pg_dump -U agentfirst -d agentfirst -Fc -f /backups/manual-$(date +%Y%m%d-%H%M).dump
```

**恢复备份：**
```bash
# 先停止 server
docker compose -f compose.prod.yaml stop server

# 恢复（替换为你的备份文件名）
docker compose -f compose.prod.yaml exec db pg_restore -U agentfirst -d agentfirst -c /backups/agentfirst-20260828-1200.dump

# 重启服务
docker compose -f compose.prod.yaml start server
```

### 6.5 查看数据库

```bash
# 进入数据库
docker compose -f compose.prod.yaml exec db psql -U agentfirst -d agentfirst

# 查看表
\dt

# 退出
\q
```

---

## 七、常见问题排查

### 7.1 网站打不开

```bash
# 检查容器是否都在运行
docker compose -f compose.prod.yaml ps

# 检查端口是否监听
netstat -tlnp | grep -E '80|443'

# 检查防火墙/安全组是否放行 80/443
```

### 7.2 SSL 证书申请失败

- 确认域名已正确解析到服务器 IP
- 确认 80 端口已放行（Let's Encrypt 验证需要）
- 确认邮箱格式正确

```bash
# 查看 caddy 日志找具体原因
docker compose -f compose.prod.yaml logs caddy
```

### 7.3 数据库连接失败

```bash
# 检查 db 容器是否健康
docker compose -f compose.prod.yaml ps

# 查看 db 日志
docker compose -f compose.prod.yaml logs db
```

### 7.4 Embedding 服务调用失败

```bash
# 查看 server 日志
docker compose -f compose.prod.yaml logs server

# 常见原因：
# 1. API Key 错误
# 2. 模型名称不对
# 3. 接口地址不正确（必须是 OpenAI 兼容格式）
# 4. 服务器网络无法访问外部 API
```

> 系统有熔断器设计，embedding 故障时会自动降级为纯词法检索，不会影响网站使用。

### 7.5 内存不足

Rust 编译时比较耗内存，如果编译失败：

```bash
# 增加 swap 分区
fallocate -l 2G /swapfile
chmod 600 /swapfile
mkswap /swapfile
swapon /swapfile
echo '/swapfile none swap sw 0 0' >> /etc/fstab
```

---

## 八、安全加固建议

### 8.1 修改 SSH 端口

```bash
nano /etc/ssh/sshd_config
# 修改 Port 22 为其他端口，比如 Port 2222
systemctl restart sshd
```

### 8.2 禁用 root 密码登录（用密钥登录）

```bash
# 先确保你有密钥能登录，再执行
nano /etc/ssh/sshd_config
# PermitRootLogin prohibit-password
# PasswordAuthentication no
systemctl restart sshd
```

### 8.3 配置防火墙

```bash
# Ubuntu ufw
ufw allow 80/tcp
ufw allow 443/tcp
ufw allow 22/tcp
ufw enable
```

### 8.4 定期更新系统

```bash
apt update && apt upgrade -y
```

---

## 九、监控与告警（可选）

### 9.1 简单的健康检查脚本

```bash
cat > /opt/check_health.sh << 'EOF'
#!/bin/bash
if ! curl -sf https://yourdomain.com/ > /dev/null; then
    echo "网站宕机了，正在重启..."
    cd /opt/agent-first/deploy
    docker compose -f compose.prod.yaml restart server
fi
EOF

chmod +x /opt/check_health.sh

# 添加到 crontab，每 5 分钟检查一次
(crontab -l 2>/dev/null; echo "*/5 * * * * /opt/check_health.sh") | crontab -
```

---

## 十、部署检查清单

部署完成后，逐项确认：

- [ ] 域名解析正确，能通过 HTTPS 访问
- [ ] SSL 证书有效（浏览器地址栏有小锁）
- [ ] 能正常注册/登录
- [ ] 能创建 Agent 并获取 Key
- [ ] 搜索功能正常（无 embedding 也能用词法搜索）
- [ ] 写记忆功能正常
- [ ] 数据库备份目录有文件生成
- [ ] 服务器安全组只开放必要端口
- [ ] 数据库密码是强密码

---

## 附：环境变量完整列表

| 变量 | 必填 | 说明 |
|---|---|---|
| `DOMAIN` | 是 | 你的域名，如 `example.com` |
| `ACME_EMAIL` | 是 | 申请 SSL 证书的邮箱 |
| `POSTGRES_PASSWORD` | 是 | PostgreSQL 数据库密码 |
| `EMBEDDING_ENDPOINT` | 否 | Embedding API 地址（OpenAI 兼容） |
| `EMBEDDING_API_KEY` | 否 | Embedding API Key |
| `EMBEDDING_MODEL` | 否 | Embedding 模型名称 |
| `TRUSTED_PROXIES` | 否 | 可信代理网段，默认 `172.16.0.0/12` |
| `SEARCH_LEXICAL_MIN_SCORE` | 否 | 词法检索最低分，默认 0.10 |
| `SEARCH_SEMANTIC_MIN_SCORE` | 否 | 语义检索最低分，默认 0.50 |
| `SEARCH_GAP_MIN_SCORE` | 否 | 缺口检索最低分，默认 0.65 |

#!/bin/bash
# ============================================================
# ExperienceNet 腾讯云一键部署脚本
# 使用方法：
#   1. 上传整个项目到服务器 /opt/experiencenet
#   2. cd /opt/experiencenet/deploy
#   3. cp .env.example .env 并编辑填写
#   4. chmod +x deploy.sh && ./deploy.sh
# ============================================================

set -e

echo "======================================"
echo "  ExperienceNet 生产部署脚本"
echo "======================================"
echo ""

# 检查是否在 deploy 目录下
if [ ! -f "compose.prod.yaml" ]; then
    echo "❌ 请在 deploy 目录下运行此脚本"
    exit 1
fi

# 检查 .env 文件
if [ ! -f ".env" ]; then
    echo "❌ 未找到 .env 文件"
    echo "   请先执行: cp .env.example .env"
    echo "   然后编辑 .env 填写配置"
    exit 1
fi

# 检查 Docker
if ! command -v docker &> /dev/null; then
    echo "❌ 未安装 Docker，正在安装..."
    curl -fsSL https://get.docker.com | bash
    systemctl start docker
    systemctl enable docker
    echo "✅ Docker 安装完成"
else
    echo "✅ Docker 已安装: $(docker --version)"
fi

# 检查 Docker Compose
if ! docker compose version &> /dev/null; then
    echo "❌ Docker Compose 不可用"
    exit 1
else
    echo "✅ Docker Compose 已安装: $(docker compose version)"
fi

echo ""
echo "📦 开始构建并启动服务..."
echo "   首次构建需要 5-10 分钟（Rust 编译较慢）"
echo ""

# 启动服务
docker compose -f compose.prod.yaml up -d --build

echo ""
echo "⏳ 等待服务启动..."
sleep 5

# 检查状态
echo ""
echo "📊 服务状态："
docker compose -f compose.prod.yaml ps

echo ""
echo "📝 最近日志："
docker compose -f compose.prod.yaml logs --tail=20 server

echo ""
echo "======================================"
echo "  🎉 部署完成！"
echo "======================================"
echo ""
echo "  常用命令："
echo "    查看日志: docker compose -f compose.prod.yaml logs -f"
echo "    查看状态: docker compose -f compose.prod.yaml ps"
echo "    重启服务: docker compose -f compose.prod.yaml restart server"
echo "    停止服务: docker compose -f compose.prod.yaml down"
echo "    更新代码: git pull && docker compose -f compose.prod.yaml up -d --build server"
echo ""
echo "  备份文件位置: ./backups/"
echo "  每天自动备份，保留 14 天"
echo ""

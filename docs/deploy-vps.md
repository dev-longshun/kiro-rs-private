# VPS 部署指南（Docker）

## 镜像地址

```
ghcr.io/dev-longshun/kiro-rs-private:latest
```

## 前置要求

- Debian 12 或其他 Linux 发行版
- Docker 已安装

安装 Docker（如未安装）：

```bash
curl -fsSL https://get.docker.com | sh
```

## 部署步骤

### 1. 创建项目目录

```bash
mkdir -p ~/kiro-rs/data
```

### 2. 创建 docker-compose.yml

```bash
cat > ~/kiro-rs/docker-compose.yml << 'EOF'
services:
  kiro-rs:
    image: ghcr.io/dev-longshun/kiro-rs-private:latest
    container_name: kiro-rs
    extra_hosts:
      - "host.docker.internal:host-gateway"
    ports:
      - "127.0.0.1:8990:8990"
    volumes:
      - ./data:/app/config
    restart: unless-stopped
EOF
```

端口绑定 `127.0.0.1`，仅本机可访问。如需作为 New API 上游渠道，同机部署时填 `http://127.0.0.1:8990` 即可。

### 3. 创建配置文件

```bash
cat > ~/kiro-rs/data/config.json << 'EOF'
{
  "apiKey": "你的API密钥",
  "host": "0.0.0.0",
  "port": 8990,
  "adminApiKey": "你的管理后台密钥"
}
EOF
```

### 4. 拉取并启动

```bash
cd ~/kiro-rs
docker compose pull
docker compose up -d
```

### 5. 验证运行

```bash
docker compose logs -f
```

看到 `启动 Anthropic API 端点: 0.0.0.0:8990` 即为成功。

## 本地访问管理后台

端口绑定为 `127.0.0.1`，外部无法直接访问。通过 SSH 隧道将远程端口映射到本地，即可在浏览器中操作管理后台（包括添加凭据、复制等需要剪贴板的操作）。

### 方式一：命令行 SSH 隧道

```bash
ssh -L 8990:127.0.0.1:8990 -i /path/to/your/private-key root@服务器IP
```

### 方式二：Termius 端口转发

1. 左侧菜单进入 Port Forwarding
2. 新建规则，填写：
   - Local port number: `8990`
   - Bind address: `127.0.0.1`
   - Intermediate host: 选择对应服务器
   - Destination address: `127.0.0.1`
   - Destination port number: `8990`
3. 双击规则启用

隧道建立后，本地浏览器打开 `http://localhost:8990/admin` 即可访问管理后台。

## 版本标签

- `latest` — 打 `v*` tag 时更新（正式版本）
- `beta` — 每次推送到 `main` 分支时更新

## 常用运维命令

- 查看日志：`docker compose logs -f`
- 重启服务：`docker compose restart`
- 更新镜像：`docker compose pull && docker compose up -d`
- 停止服务：`docker compose down`

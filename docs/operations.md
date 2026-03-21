# 运维常用命令

## 镜像版本

- `beta` — 每次 push 到 main 自动构建，用于测试
- `latest` — 打 tag 后构建，用于生产

## 服务器部署/更新

```bash
# 进入部署目录
cd /root/kiro-rs

# 拉取最新镜像并重启
docker compose pull && docker compose up -d

# 切换镜像版本（如 latest → beta）
sed -i 's/:latest/:beta/' docker-compose.yml
docker compose pull && docker compose up -d
```

## 日志查看

```bash
# 实时跟踪日志（Ctrl+C 退出，不影响容器运行）
docker logs -f kiro-rs

# 最近 200 行
docker logs --tail 200 kiro-rs

# 最近 10 分钟
docker logs --since 10m kiro-rs

# 过滤关键错误
docker logs kiro-rs 2>&1 | grep -E "失败|429|401|403|冷却|禁用|刷新失败"
```

## 容器管理

```bash
# 查看容器状态
docker ps | grep kiro-rs

# 查看当前使用的镜像
docker inspect kiro-rs --format='{{.Config.Image}}'

# 重启容器
docker compose restart

# 停止容器
docker compose down

# 清空日志后重启（便于追踪问题）
truncate -s 0 $(docker inspect --format='{{.LogPath}}' kiro-rs)
docker restart kiro-rs
```

## CI 构建

```bash
# 查看最近构建状态
gh run list -R dev-longshun/kiro-rs-private --limit 5

# 查看失败构建的日志
gh run view <run_id> -R dev-longshun/kiro-rs-private --log-failed

# 手动触发构建
gh workflow run docker-build.yaml -R dev-longshun/kiro-rs-private
```

## 本地开发

```bash
# 编译检查
cargo check

# 运行测试
cargo test

# admin-ui 本地开发
cd admin-ui && pnpm install && pnpm dev

# admin-ui 构建
cd admin-ui && pnpm build
```

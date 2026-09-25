# 部署文件说明

本目录的三个模板面向 Linux 生产环境：C++ 服务只监听 loopback 的 `8080`，Nginx 负责 TLS/静态文件/反向代理，systemd 负责进程生命周期。它们不是开发平台限制：macOS、Linux、Windows 都应能本地构建、运行单元测试和连接开发 MySQL；Windows 生产可用 Windows Service + IIS/Nginx，macOS 通常仅用于开发。`docker-compose.yml` 适合本地依赖服务，不应把生产密码提交到 Git。

## 本地依赖

```bash
docker compose -f docs/deploy/docker-compose.yml up -d
cmake -S . -B build -DCMAKE_BUILD_TYPE=Debug
cmake --build build
./build/ecshop_api
```

## 生产步骤

1. 创建不可登录账户 `ecshop` 和 `/opt/cpp-ecshop`；将编译产物、只读配置和写入目录分开。
2. 为 MySQL 创建最小权限账户，执行 `docs/sql/001_core_schema.mysql8.sql` 和后续 migration。
3. 把 `ecshop-api.service` 中的路径、环境文件、用户改为实际值，`systemctl enable --now ecshop-api`。
4. 安装 `nginx.conf` 的 server 块，配置证书，`nginx -t && systemctl reload nginx`。
5. 配置备份、日志轮转、健康检查与告警；发布采用新二进制 + `systemctl restart`，失败可回滚上个二进制。

不要把 `.env`、数据库备份、支付密钥或 Cookie 放进镜像、代码库或日志。

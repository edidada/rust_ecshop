# 第一个 URL：商品详情

已实现 `GET /api/v1/goods/{id}`。它对应 PHP 项目的 `goods.php?id=12`，但改为 JSON API，并只返回上架商品。

## 目录如何阅读

```text
include/ecshop/domain/Goods.h                    # 纯业务数据
include/ecshop/domain/GoodsRepository.h          # 数据来源的抽象接口
src/infrastructure/InMemoryGoodsRepository.cpp   # 临时数据；下一步替换为 MySQL
src/application/GetGoodsUseCase.cpp              # 查询规则
src/http/GoodsRoutes.cpp                         # URL、HTTP 状态、JSON
src/app/main.cpp                                 # 依赖装配与启动
```

请求 `GET /api/v1/goods/12` 返回商品 JSON。`GET /api/v1/goods/13` 返回 404，因为示例将它标记为下架；`GET /api/v1/goods/0` 返回 400。价格是字符串，刻意不使用 `double`。

## 本机运行

```bash
cmake -S . -B build -G Ninja
cmake --build build
ctest --test-dir build --output-on-failure
./build/ecshop_api
```

另开一个终端执行：

```bash
bash scripts/curl_goods_example.sh
```

## 写下一个 URL 时照抄

1. 在 `domain` 定义实体和值对象，不能依赖 Crow 或 JSON。
2. 在 `domain` 新增 Repository 接口，先写内存实现和单元测试。
3. 在 `application` 写一个只做一个动作的 Use Case。
4. 在 `http` 增加路由，验证参数并转 JSON。
5. 为正常、参数错误、资源不存在各写一个 curl 测试。

下一个推荐实现：`GET /api/v1/categories/{id}/goods?page=1&page_size=20`。它仍是只读功能，但能练习 query 参数、分页 DTO 和列表 JSON。

本示例锁定项目内的 `third_party/cpp-httplib/httplib.h`，不依赖 Homebrew 或 vcpkg，因此 macOS 12、Linux 与 Windows 可用同一 CMake 命令构建。`vcpkg.json` 保留了 MySQL、日志、密码等后续阶段的依赖清单；数据库 URL 再引入它们。

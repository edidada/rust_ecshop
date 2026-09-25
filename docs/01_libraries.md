# C++ 项目库与构建信息

## 三平台基线（macOS / Linux / Windows）

第一原则是：业务代码只依赖 ISO C++、CMake target 与抽象接口；不要引入 `fork`、`epoll`、`pthread`、`unistd.h`、systemd API 或 Linux 文件路径。三端均用 CMake 构建，依赖统一优先通过 **vcpkg manifest mode** 管理；macOS 可使用 AppleClang，Linux 使用 GCC/Clang，Windows 使用 MSVC。CI 应至少矩阵验证 `macos-latest`、`ubuntu-latest`、`windows-latest`。

当前首个 URL 使用 cpp-httplib：官方将其定位为 C++11 单文件、跨平台 HTTP/HTTPS 服务端和客户端库。它不依赖 Homebrew 或 vcpkg，尤其适合 macOS 12 的入门环境；未来若需要更完整中间件，再评估 Crow。[cpp-httplib](https://github.com/yhirose/cpp-httplib)

## 建议的第一版组合

| 领域 | 选择 | 为什么 | CMake 目标/头文件 |
| --- | --- | --- |
| HTTP 路由（当前） | cpp-httplib | 单头文件、跨平台、没有额外安装链，先专注 HTTP | `httplib.h` |
| HTTP 路由（后续可选） | Crow | 路由、JSON、中间件更完整；包管理环境稳定后再引入 | `Crow::Crow` / `crow.h` |
| JSON | nlohmann/json | 值类型 API 易读，序列化 DTO 很直观 | `nlohmann_json::nlohmann_json` / `nlohmann/json.hpp` |
| MySQL | MySQL Connector/C++（classic JDBC API） | 与原 ECSHOP 的关系模型直接匹配；只经 Repository 层使用 | `mysql::concpp` |
| 日志 | spdlog | 格式化日志与异步日志能力成熟 | `spdlog::spdlog` |
| 密码 | libsodium | 使用 `crypto_pwhash` 保存密码哈希，不保存明文或 MD5 | `sodium` |
| 配置 | dotenv-cpp 或小型自写解析器 | `.env` 只在开发读取，生产交给环境变量 | 仅 Infrastructure 层 |
| 测试 | Catch2 + CTest | 单元/集成测试分层，命令统一 | `Catch2::Catch2WithMain` |

所有上述库都必须在三端的 CI 中实际编译；不能只因为本机 macOS 能链接就视为跨平台。MySQL Connector/C++ 的 CMake 用法与版本支持见其官方说明；目标服务器与本地开发系统不同时，Connector 的动态库/插件也要随部署验证。[Connector/C++ 官方文档](https://dev.mysql.com/doc/dev/connector-cpp/latest/usage.html)

CMake 的官方教程覆盖 target、`find_package()`、CTest 与安装，适合作为本项目的构建学习主线。[CMake 教程](https://cmake.org/cmake/help/latest/guide/tutorial/index.html)

MySQL Connector/C++ 的现代 CMake 目标为 `mysql::concpp`，可通过 `find_package(mysql-concpp REQUIRED)` 导入；它支持传统 SQL 和 X DevAPI。此项目第一版应选择 classic JDBC API 或直接采用参数化 SQL 封装，避免把业务建模为 Document Store。[官方使用说明](https://dev.mysql.com/doc/dev/connector-cpp/latest/usage.html)

## 不建议一开始引入

- 不要同时使用 cpp-httplib、Crow 与 Drogon：先掌握一个 HTTP 框架。
- 不要将 SQL 写进 Handler：连接、事务、语句和行映射只在 Repository。
- 不要在业务中直接读取环境变量：在启动时构造不可变 `AppConfig`。
- 不要使用 ORM 来掩盖 SQL 学习；先手写少量参数化 SQL，再评估 ORM。

## 目标目录

```text
src/
  app/                 # main、依赖装配、配置
  http/                # Router、Middleware、Handler、DTO
  domain/              # 实体、值对象、状态机、业务服务接口
  application/         # Use case：PlaceOrder、AddCartItem 等
  infrastructure/      # MySQL Repository、Redis、密码/时钟实现
  shared/              # Error、Result、分页、日志
tests/
  unit/ integration/
```

## 最小 CMake 形状（待实现时采用）

```cmake
cmake_minimum_required(VERSION 3.25)
project(cpp_ecshop LANGUAGES CXX)
set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
find_package(Crow CONFIG REQUIRED)
find_package(nlohmann_json CONFIG REQUIRED)
find_package(spdlog CONFIG REQUIRED)
find_package(mysql-concpp REQUIRED)

add_executable(ecshop_api src/app/main.cpp)
target_link_libraries(ecshop_api PRIVATE Crow::Crow nlohmann_json::nlohmann_json
 spdlog::spdlog mysql::concpp)
```

说明：当前 `CMakeLists.txt` 已采用 C++20，并为首个 URL 配置项目内 cpp-httplib；上例是后续引入数据库、日志与 JSON 库时的扩展形态。

## 跨平台构建命令

先安装 vcpkg 并设置 `VCPKG_ROOT`，项目根目录实施 `vcpkg.json` 后使用同一种 CMake 调用；不要把某个平台的 Homebrew、apt 或 Visual Studio include 路径硬编码进 `CMakeLists.txt`。

```bash
# macOS / Linux
cmake -S . -B build -DCMAKE_BUILD_TYPE=Debug \
  -DCMAKE_TOOLCHAIN_FILE="$VCPKG_ROOT/scripts/buildsystems/vcpkg.cmake"
cmake --build build --parallel
ctest --test-dir build --output-on-failure

# Windows PowerShell
cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Debug `
  -DCMAKE_TOOLCHAIN_FILE="$env:VCPKG_ROOT/scripts/buildsystems/vcpkg.cmake"
cmake --build build --parallel
ctest --test-dir build --output-on-failure
```

可在 M0 创建如下 manifest（版本基线锁定到 `vcpkg-configuration.json`，由 CI 固定 commit）：

```json
{
  "name": "cpp-ecshop",
  "version-string": "0.1.0",
  "dependencies": ["crow", "nlohmann-json", "spdlog", "catch2", "libsodium"]
}
```

数据库驱动在各环境可能由 MySQL 官方包或 vcpkg 提供，故将其安装与发现封装成 `cmake/Dependencies.cmake`；业务 target 永远链接自定义的 `ecshop::mysql` interface target，而不是硬编码某个操作系统的库文件名。

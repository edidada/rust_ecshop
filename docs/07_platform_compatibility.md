# macOS、Linux、Windows 兼容性清单

## 支持边界

| 项目 | macOS | Linux | Windows | 约束 |
| --- | --- | --- | --- | --- |
| 编译器 | AppleClang | GCC / Clang | MSVC | 开启 `-Wall -Wextra -Wpedantic` 或 `/W4`；CI 将警告视为错误 |
| 构建 | CMake + Ninja | CMake + Ninja | CMake + Ninja / Visual Studio | 不写平台绝对路径 |
| 包管理 | vcpkg；Homebrew 仅开发便利 | vcpkg；系统包仅开发便利 | vcpkg manifest | CMake 只消费 imported targets |
| HTTP | Crow | Crow | Crow | HTTP 代码只在 `src/http` |
| 数据库 | MySQL Connector/C++ | MySQL Connector/C++ | MySQL Connector/C++ | Repository 抽象隔离驱动 |
| 测试 | CTest + Catch2 | CTest + Catch2 | CTest + Catch2 | 三端均跑 unit test；Linux 再跑 Docker integration |
| 生产托管 | 非主要目标 | systemd + Nginx | Windows Service + IIS/Nginx | Host 生命周期实现不可进入 domain |

## C++ 代码规则

- 使用 `<filesystem>`、`std::chrono`、`std::jthread`/`std::thread`、`std::mutex` 等标准库；不要直接使用 `dirent.h`、`unistd.h`、`sys/socket.h` 或 Win32 API。
- 文件路径都由 `std::filesystem::path` 传递，不手写 `/` 或 `\\`；配置中只保存相对路径或从运行时目录派生。
- 网络、信号、后台进程等系统能力收敛为小接口，例如 `IClock`、`IFileStore`、`IProcessTerminator`；Windows/Linux 特定实现放在 infrastructure 的独立编译单元。
- 使用 UTF-8 源码、UTF-8 JSON 和 MySQL `utf8mb4`。Windows 构建启用 `/utf-8`；不能假设 `wchar_t` 宽度或本地代码页。
- 避免 `long`、`size_t` 用于跨系统持久化 ID；数据库 ID 用 `std::int64_t`，时间统一 UTC。

## CI 最小矩阵

```yaml
strategy:
  matrix:
    os: [macos-latest, ubuntu-latest, windows-latest]
steps:
  - checkout
  - setup-vcpkg-cache
  - cmake-configure-with-vcpkg-toolchain
  - cmake-build
  - ctest
```

Linux 额外运行 MySQL/Redis 容器的 integration test。每次增加第三方库、编译选项或平台特定代码，必须在三个 job 都通过后才能合并。

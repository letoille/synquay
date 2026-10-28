# Synquay 架构入口

项目采用 Rust 后端与 React + TypeScript 前端，自托管优先。前端使用 Vite，后续是否引入 Next.js 再按实际需求决定。

资料空间保存详细设计：

- [架构总览](https://chatgpt.com/space/page_6adb312ddab481919b01debc98bff1bf)
- [项目目标与总体架构](https://chatgpt.com/space/page_6adb336a1a6c819198c66ce9d4271752)
- [连接适配器、MCP 桥接与内容发布](https://chatgpt.com/space/page_6adb337c27fc81919a193043c9339a43)
- [OAuth、认证、权限与审计](https://chatgpt.com/space/page_6adb338a96c881919e244f9a9023b84b)
- [事件、消息转发与可靠交付](https://chatgpt.com/space/page_6adb339df1b08191a5695788ed9bf553)
- [Rust 实现方案、管理台与交付路线](https://chatgpt.com/space/page_6adb33aa59d08191ab277274d9c10555)

## 架构方向（2026-10-28）

Synquay 定位为连接、消息路由与 MCP 能力网关；模型推理、Agent 的启动与生命周期由外部系统负责。路由使用通用连接、事件、转换与能力，不再新增 Agent 专用处理模式。Agent 专用路由和收件箱已移除。详细边界和当前实现见[架构收敛方案](architecture/scope-convergence.md)。

## 核心架构补全

[架构设计基线 v0.4](architecture/README.md) 已逐模块定义职责、实体、trait、状态机、事务与验收条件，原覆盖 15 个模块；Agent 专用模块已移除。

- [模块间对接与原子提交契约](architecture/contracts.md)
- [实施路线与现有代码迁移](architecture/roadmap.md)

目标契约与已实现功能分开记录；新增设计尚不代表运行时已支持。

## 开发边界

后端先以一个服务包验证协议和调用链，再按模块边界拆出独立 crate；避免为尚未实现的能力建立空包。

连接器应区分工具调用、事件来源和消息投递能力。CLI 执行、凭据管理与 OAuth 等需要安全边界的能力按设计逐项实现，不能用任意 shell 执行或未授权 HTTP 入口替代。

对 MCP 客户端的通知、后台唤醒与任务能力逐一验证。服务端可持久化事件，不承诺所有 Agent 平台都能被主动唤醒。

连接操作、指令、资源状态与近期消息的统一边界见 [连接能力与近期消息](architecture/connection-abilities.md)。 MCP 内置工具的发布、范围和审计设计见 [MCP 内置工具](architecture/builtin-tools.md)。

## 代码实现说明

- [连接器抽象与接入契约](connectors.md)
- [业务模块抽象、装配与调用链](module-integration.md)

应用服务已串联工具调用、事件转发和原路回复；SQLite、HTTP webhook/sink 和单路由 worker 已接通，其他协议驱动与 OAuth 设施仍需实现。

运行说明见 [持久化 HTTP 转发](http-relay.md)。

# 计划:用 Rust 编写 GitHub Copilot → Anthropic 网关 (供 Claude Code 使用)

## 问题陈述
我拥有 GitHub Copilot 权限和多个模型。需要一个全新的、用 Rust 编写的可执行程序 (exe),
它能:
1. 通过 GitHub OAuth **设备授权流**登录,获取访问权限并换取 Copilot token。
2. 拉取有权限的模型列表。
3. 暴露一个 **Anthropic 兼容的 HTTP API** (`/v1/messages`),让 Claude Code CLI
   (通过设置 `ANTHROPIC_BASE_URL`) 直接调用,把请求转译给 Copilot 的 OpenAI 风格后端,
   再把响应转译回 Anthropic 格式。

## 确认的需求 (来自用户)
- 目标接口:**Anthropic 兼容** (`/v1/messages`),供 Claude Code 使用。
- 技术栈:**axum + tokio + reqwest + serde** (+ 辅助库)。
- 响应:**流式 (SSE) 与非流式都支持,以流式为主**。
- 功能范围:**工具调用 (tool_use) + `/v1/messages/count_tokens` + 多模型列表** 全部实现。
- 登录:**GitHub 设备授权流**,token 持久化到本地文件 (无需手动复制)。

## 文档与产物位置
所有技术方案 / plan / 文档都放入新项目文件夹 **`D:\Tools\copilot-gateway`**:
- `D:\Tools\copilot-gateway\docs\PLAN.md` — 本计划副本 (技术方案主文档)。
- 后续设计/接入说明也放在 `docs\` 下;`README.md` 放项目根。
- (会话目录的 plan.md 仅作进度跟踪镜像。)

## 技术架构

```
┌─────────────┐   Anthropic /v1/messages   ┌───────────────────────────┐   OpenAI /chat/completions   ┌────────────────────┐
│ Claude Code │ ─────────────────────────▶ │   Rust Gateway (本程序)    │ ──────────────────────────▶ │ GitHub Copilot API │
│   CLI       │ ◀───────────────────────── │  axum + tokio + reqwest    │ ◀────────────────────────── │ api.githubcopilot  │
└─────────────┘    SSE / JSON (Anthropic)   └───────────────────────────┘    SSE / JSON (OpenAI)        └────────────────────┘
                                                    │
                                                    ├─ GitHub 设备授权流 (一次性登录)
                                                    └─ 本地持久化: github_token, 缓存 copilot_token
```

### 认证与 token 生命周期
1. **设备授权流** (子命令 `auth`):
   - `POST https://github.com/login/device/code` (client_id=`Iv1.b507a08c87ecfe98`, scope=`read:user`)
     → 得到 `user_code` / `verification_uri` / `device_code` / `interval`。
   - 在终端显示 user code 和验证 URL,提示用户在浏览器授权。
   - 轮询 `POST https://github.com/login/oauth/access_token`
     (grant_type=`urn:ietf:params:oauth:grant-type:device_code`) 直到拿到 `access_token` (GitHub token)。
   - 将 GitHub token 持久化到本地 (如 `~/.copilot-gateway/github_token`,权限 0600)。
2. **换取 Copilot token** (启动时 + 定时刷新):
   - `GET https://api.github.com/copilot_internal/v2/token` (header `authorization: token <github_token>`)
     → 返回 `{ token, expires_at, refresh_in }`。
   - 用 `refresh_in` 设定后台定时刷新任务 (tokio interval),保证 token 不过期。
3. **账户类型 / base url**:
   - individual → `https://api.githubcopilot.com`;企业/组织 → `https://api.<accountType>.githubcopilot.com`。

### 必需的请求头 (复刻参考实现,缺失会被 Copilot 拒绝)
- Copilot 调用:`Authorization: Bearer <copilotToken>`、`copilot-integration-id: vscode-chat`、
  `editor-version: vscode/<ver>`、`editor-plugin-version: copilot-chat/<ver>`、
  `user-agent: GitHubCopilotChat/<ver>`、`openai-intent: conversation-panel`、
  `x-github-api-version: 2025-04-01`、`x-request-id: <uuid>`、
  `x-vscode-user-agent-library-version: electron-fetch`;视觉请求加 `copilot-vision-request: true`;
  Agent 场景加 `X-Initiator: agent`(消息含 assistant/tool 时),否则 `user`。
- VS Code 版本号:可硬编码一个已知版本,或启动时从公开接口获取 (参考有 `get-vscode-version`)。

## HTTP 接口 (axum 路由)
- `GET  /` → 健康检查。
- `GET  /v1/models` → 返回模型列表 (从 Copilot `/models` 拉取并缓存,可按需转成 Anthropic 风格)。
- `POST /v1/messages` → **核心**。Anthropic 请求 → OpenAI 请求 → 调 Copilot → 响应转回 Anthropic
  (流式走 SSE,非流式走 JSON)。
- `POST /v1/messages/count_tokens` → 估算 token 数 (用 tokenizer 近似)。
- (可选) `GET /token`、`GET /usage` 便于调试。

## 关键难点:Anthropic ↔ OpenAI 转译

### 请求:Anthropic → OpenAI (非流式逻辑通用)
- `system` (字符串或 block 数组) → OpenAI `system`/`developer` 消息。
- `messages[]`:Anthropic 的 content block (`text` / `image` / `tool_use` / `tool_result`)
  → OpenAI 的 `content` (string 或 parts) + `tool_calls` / `role:tool` 消息。
  - `tool_use` block → OpenAI assistant `tool_calls` (function.name + arguments JSON)。
  - `tool_result` block → OpenAI `role:"tool"` 消息 (tool_call_id + content)。
  - `image` block (base64/url) → OpenAI `image_url` part,并触发 vision header。
- `tools[]` (Anthropic input_schema) → OpenAI `tools[]` (function.parameters)。
- `tool_choice`:`auto`/`any`/`tool` → OpenAI `auto`/`required`/具体函数。
- `max_tokens` / `temperature` / `top_p` / `stop_sequences` → 对应字段。
- `model`:解析为 Copilot 实际接受的 id(用户覆盖 → 透传合法 id → 按 opus/sonnet/haiku 家族归一 → 原样),见 `model_map.rs`。

### 响应:OpenAI → Anthropic (非流式)
- OpenAI `choices[0].message` → Anthropic `content[]`:
  - `content` 文本 → `text` block;`tool_calls` → `tool_use` block (解析 arguments JSON)。
- `finish_reason` (`stop`/`length`/`tool_calls`) → Anthropic `stop_reason`
  (`end_turn`/`max_tokens`/`tool_use`)。
- `usage` → Anthropic `usage` (`input_tokens` / `output_tokens`)。
- 顶层填充 `id` / `type:"message"` / `role:"assistant"` / `model`。

### 响应:OpenAI 流 → Anthropic SSE 事件序列 (最复杂)
需维护流状态机,按 Anthropic 事件协议产出有序事件:
1. `message_start` (含初始 usage、空 content)。
2. 对每个 content block:`content_block_start` → 多个 `content_block_delta` → `content_block_stop`。
   - 文本增量 → `text_delta`;工具调用参数增量 → `input_json_delta` (累积 `arguments` 片段)。
3. `message_delta` (携带最终 `stop_reason` 与 `usage.output_tokens`)。
4. `message_stop`。
- 状态:`message_start` 是否已发、当前 block index、block 是否打开、各 tool_call 的累积缓冲。
- 处理 OpenAI 的 `[DONE]` 终止标记。
- 参考实现:`stream-translation.ts` / `non-stream-translation.ts` (仅作协议参考,不复制代码)。

## 项目结构 (建议放在 `D:\Tools\copilot-gateway`)
```
copilot-gateway/
├── Cargo.toml
├── README.md
└── src/
    ├── main.rs              # CLI 入口 (clap 子命令: auth / start)
    ├── config.rs            # 路径、版本常量、token 文件读写、model_map 路径
    ├── state.rs             # 共享状态 (copilot_token, models, account_type, model_map)
    ├── model_map.rs         # 模型别名解析 (用户覆盖/透传/家族归一)
    ├── auth/
    │   ├── device_code.rs   # 设备码请求
    │   └── poll_token.rs     # 轮询 access_token
    ├── github/
    │   └── copilot_token.rs # 换取/刷新 copilot token
    ├── copilot/
    │   ├── models.rs        # 拉取模型列表
    │   └── chat.rs          # 调 /chat/completions (流式+非流式)
    ├── server/
    │   ├── mod.rs           # axum router + 启动
    │   ├── messages.rs      # /v1/messages handler
    │   ├── count_tokens.rs  # /v1/messages/count_tokens
    │   └── models.rs        # /v1/models
    ├── translate/
    │   ├── anthropic_types.rs  # Anthropic 请求/响应/事件结构 (serde)
    │   ├── openai_types.rs     # OpenAI 请求/响应/chunk 结构 (serde)
    │   ├── request.rs          # Anthropic → OpenAI
    │   ├── response.rs         # OpenAI → Anthropic (非流式)
    │   └── stream.rs           # OpenAI chunk → Anthropic SSE 事件 (状态机)
    └── tokenizer.rs         # count_tokens 估算 (tiktoken-rs 或近似)
```

## 依赖 (Cargo.toml)
- `tokio` (rt-multi-thread, macros)、`axum` (含 SSE)、`reqwest` (json, stream)、
  `serde` / `serde_json`、`clap` (CLI 子命令)、`uuid` (x-request-id)、
  `anyhow` / `thiserror` (错误)、`tracing` / `tracing-subscriber` (日志)、
  `futures` / `tokio-stream` / `eventsource-stream` (解析上游 SSE)、
  `directories` (定位配置目录)、`tiktoken-rs` (count_tokens,可选)。

## 实施步骤 (todos)
1. **项目脚手架**:创建 Cargo 项目、依赖、CLI 骨架 (`auth` / `start`)。
2. **设备授权流**:device_code + 轮询 access_token + token 持久化。
3. **Copilot token**:换取 + 后台定时刷新 + 账户类型/base url 处理。
4. **请求头与 HTTP 客户端**:统一构造 copilot/github headers。
5. **模型列表**:拉取 + 缓存 + `/v1/models` 路由。
6. **类型定义**:Anthropic 与 OpenAI 的 serde 结构体。
7. **请求转译**:Anthropic → OpenAI (含 tool_use / tool_result / image / tools)。
8. **非流式响应转译**:OpenAI → Anthropic (含 tool_use / stop_reason / usage)。
9. **流式转译状态机**:OpenAI chunk → Anthropic SSE 事件序列。
10. **axum 服务**:`/v1/messages` (流式+非流式) 接线、错误处理、CORS、日志。
11. **count_tokens**:`/v1/messages/count_tokens` 估算。
12. **端到端验证**:用 Claude Code (`ANTHROPIC_BASE_URL=http://localhost:<port>`) 实测对话与工具调用。
13. **打包与文档**:`cargo build --release` 生成 exe;README 写明 auth/start/接入步骤。

## 验证方式
- 单元测试:转译函数 (请求/非流式响应/流式事件序列) 用样例 JSON 断言。
- 集成测试:对 `/v1/messages` 发送 Anthropic 样例请求,校验返回结构。
- 真机:配置 Claude Code 指向网关,验证普通对话、流式输出、工具调用全链路。

## 注意事项 / 风险
- Copilot 请求头需精确复刻,否则被拒 (401/403)。
- 流式状态机是最易出错处,需重点测试 (文本块与工具块交错、参数分片累积)。
- `model` id 需是 Copilot 实际支持的;可在 `/v1/models` 暴露真实 id 供 Claude Code 选择。
- token 文件含敏感信息,需设置严格文件权限,且不得提交到仓库。
- VS Code 版本号若过旧可能被服务端拒绝,需保持可配置/可更新。

## 实现状态 (已完成)
代码已落地于 **`D:\Tools\copilot-gateway`**,`cargo build --release` 通过,`cargo clippy` 干净,15 个自动化测试全部通过。

- ✅ Cargo 脚手架 + CLI (`auth` / `start`,clap)。
- ✅ 设备授权流 + token 持久化 (`~/.copilot-gateway/github_token`,Unix 0600)。
- ✅ Copilot token 换取 + 后台定时刷新 + 账户类型 base url。
- ✅ 精确复刻 Copilot/GitHub 请求头 (含 X-Initiator / vision)。
- ✅ 模型列表 (`/v1/models`,带缓存)。
- ✅ 模型别名解析 (`model_map.rs`):用户覆盖 (`~/.copilot-gateway/model_map.json`) → 透传合法 id → opus/sonnet/haiku 家族归一 → 原样;使 Claude Code `/model` 菜单选预设也能用。
- ✅ 401 重新认证提示:GitHub token 失效 (换 token 或聊天上游返回 401) 时日志提示重跑 `copilot-gateway auth`。
- ✅ Anthropic/OpenAI serde 类型 (含 thinking / 未知块容错)。
- ✅ 请求转译 (system / text / image / tool_use / tool_result / tools / tool_choice)。
- ✅ 非流式响应转译 (text / tool_use / stop_reason / usage)。
- ✅ 流式状态机 (message_start → content_block_* → message_delta → message_stop;
  仅在 id+name 就绪时开启工具块;顺序索引;上游错误发 `error` 事件)。
- ✅ axum 服务接线 (`/v1/messages` 流式+非流式、统一错误响应)。
- ✅ `/v1/messages/count_tokens` (cl100k_base 近似)。
- ✅ 单元/集成测试:转译 (文本流、单/多工具流、thinking 容错、非流式工具) + 模型别名解析 (6 例) + 健康路由。
- ✅ README + .gitignore。
- ✅ 端到端 (Claude Code 实测):GitHub 登录后,设 `ANTHROPIC_BASE_URL=http://127.0.0.1:4141` 运行 `claude`;非流式与流式真实调用 (claude-opus-4.8) 及别名映射 (claude-opus-4-1-20250805 → claude-opus-4.8) 均验证通过。

### Rubber-duck 复盘要点 (已采纳)
1. 工具块仅在 `id`+`name` 同时就绪时才发 `content_block_start`,避免空 name / 伪造 id。
2. 移除"重开已停止块"逻辑,改为镜像参考实现的顺序单块模型。
3. 先读取 `chunk.usage` 再发 `message_start`,并在 `message_delta` 回报 input/output tokens。
4. 新增 `thinking` 折叠为文本 + `#[serde(other)]` 容错未知块,避免反序列化失败。
5. 上游流错误改为发送 Anthropic `error` 事件,不再伪造正常 `end_turn`。

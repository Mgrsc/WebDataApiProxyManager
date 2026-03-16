# 🌐 Web Data API Proxy Manager (WDAPM)

[English](./README.md)

**Web Data API Proxy Manager (WDAPM)** 是一个专为 Web Data API 设计的**统一代理与管理平台**。它将 Exa、Tavily、Firecrawl、Jina 等主流数据提供商（Provider）的能力收敛至单一入口，助你轻松实现账号、密钥、网络代理、请求日志与告警的一站式管理。

## 🖼️ 界面预览

<table>
  <tr>
    <td width="50%">
      <img src="./docs/images/overview.png" alt="Overview" width="100%">
    </td>
    <td width="50%">
      <img src="./docs/images/accounts.png" alt="Accounts" width="100%">
    </td>
  </tr>
  <tr>
    <td width="50%">
      <img src="./docs/images/request.png" alt="Request Logs" width="100%">
    </td>
    <td width="50%">
      <img src="./docs/images/settings.png" alt="Settings" width="100%">
    </td>
  </tr>
</table>

## ✨ 核心特性

* **🔌 统一接入入口**：业务侧只需对接 WDAPM，轻松调用多家 Web Data API。
* **🔑 集中密钥管理**：后台统一管理 Provider 账号，便于多账号轮换与日常运营。
* **🛡️ 平台 API Key 分发**：支持发放内部使用的 API Key，避免直接暴露上游真实密钥。
* **🌐 独立出口代理 (Egress Proxy)**：支持为不同账号绑定独立的出口代理，轻松实现 IP/区域隔离，规避上游风控。
* **📊 全面监控与日志**：内置请求日志、异步任务状态监控、审计记录与基础报表，问题排查更直观。
* **🔔 健康告警机制**：支持自定义告警规则与事件记录，持续监控账号与请求的健康状态。
* **💻 开箱即用的 Web UI**：自带可视化管理控制台，部署后直接在浏览器中完成初始化与日常运维。

---

## 🚀 Docker 快速部署

> 💡 **提示**：如果你需要使用 Firecrawl 的**异步任务回调**功能，请提前准备一个公网可访问的域名，并在稍后的 `.env` 文件中配置 `WDAPM_WEBHOOK_BASE_URL`。

**1. 创建目录并下载配置文件**

```bash
mkdir -p wdapm && cd wdapm
curl -fsSL -o docker-compose.yml https://raw.githubusercontent.com/Mgrsc/WebDataApiProxyManager/main/docker-compose.yml
curl -fsSL -o .env.example https://raw.githubusercontent.com/Mgrsc/WebDataApiProxyManager/main/.env.example
```

**2. 准备并修改环境变量**

```bash
cp .env.example .env
```

打开 `.env` 文件并按需修改。一般只需关注 `WDAPM_WEBHOOK_BASE_URL`（仅用于 Firecrawl 异步 webhook 场景，填入你的公网访问地址）。

**3. 拉取镜像并启动服务**

```bash
docker compose pull
docker compose up -d
```

**4. 访问控制台**

服务启动后，通过浏览器访问默认地址即可：

```text
http://<你的服务器IP>:8080
```

---

## 🛠️ 初始化与最佳实践

首次打开控制台时，系统会引导你**设置管理员密码**并自动生成一个**默认的平台 API Key**。请妥善保存该 Key，随后建议按以下步骤完成基础配置：

1. **添加上游账号**：录入你的 Exa / Tavily / Firecrawl / Jina 等账号信息。
2. **绑定出口代理**：为有需要的账号配置 Egress Proxy。
3. **设置告警规则**：配置监控告警以便及时发现异常。

> ⚠️ **关于 IP 风控的重要提示**
> 如果未给 Provider 账号绑定出口代理，WDAPM 会直接使用本机 IP 请求上游。在多账号共用单机 IP、高频调用或跨区域使用的场景下，极易触发上游的风控策略（如限流、验证码拦截、账号异常甚至封禁）。
> **强烈建议：** 当账号数量较多或对稳定性要求较高时，务必为各账号配置**独立的出口代理 (Egress Proxy)**，做好 IP 隔离。

---

## 🎯 业务侧接入

只需将原本请求上游的 Base URL 替换为 WDAPM 的服务地址，并在请求头中携带你的**平台 API Key**，即可访问对应路径：

* 代理 Exa： `/exa/...`
* 代理 Tavily： `/tavily/...`
* 代理 Firecrawl： `/firecrawl/...`
* 代理 Jina Reader： `/jina/r/...`
* 代理 Jina Search： `/jina/s/...`

### Jina 路由映射说明

Jina 的接入方式和 Exa、Tavily 这类常规 API 略有不同。WDAPM 会把 Jina 上游域名里的前缀映射到路径中：

* `https://r.jina.ai/...` -> `/jina/r/...`
* `https://s.jina.ai/...` -> `/jina/s/...`

例如：

```text
原始上游请求：
https://r.jina.ai/http://example.com

通过 WDAPM：
http://<你的服务器IP>:8080/jina/r/http://example.com
```

```text
原始上游请求：
https://s.jina.ai/http://example.com

通过 WDAPM：
http://<你的服务器IP>:8080/jina/s/http://example.com
```

> 注意：Jina 路由必须以 `r/` 或 `s/` 开头，像 `/jina/...` 这种未带前缀的写法是无效的。

---

## ⚡ 并发性能建议

以下数据基于本地测试环境（单机部署 + SQLite + Mock 上游），仅供**容量规划参考**，不作为严格的 SLA 承诺：

* **推荐运行区间**：持续稳定运行的理想并发量约为 **192 - 256**。
* **预估吞吐量**：在推荐并发下，约可处理 **12,000 - 18,000 req/min**。
* **饱和极值**：总并发达到 `384` 时系统已进入明显饱和区（可维持 30 分钟，但不建议作为常规负载，因为我只测试了30分钟）。

---

## 📄 License

本项目采用 [Apache-2.0](./LICENSE) 许可协议。

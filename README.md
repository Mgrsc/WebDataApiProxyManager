# 🌐 Web Data API Proxy Manager (WDAPM)

[中文](./README_zh.md)

**Web Data API Proxy Manager (WDAPM)** is a **unified proxy and management platform** designed specifically for Web Data APIs. It consolidates the capabilities of mainstream data providers such as Exa, Tavily, Firecrawl, and Jina into a single entry point, helping you achieve one-stop management of accounts, keys, egress proxies, request logs, and alerts.

## 🖼️ Interface Preview

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

## ✨ Core Features

* **🔌 Unified Entry Point**: The client side only needs to interface with WDAPM to easily call multiple Web Data APIs.
* **🔑 Centralized Key Management**: Manage provider accounts via a unified backend, facilitating multi-account rotation and daily operations.
* **🛡️ Platform API Key Distribution**: Support for issuing internal API keys to avoid direct exposure of upstream provider keys.
* **🌐 Independent Egress Proxy**: Supports binding independent egress proxies to different accounts, enabling IP/regional isolation to circumvent upstream risk controls.
* **📊 Comprehensive Monitoring & Logs**: Built-in request logs, async task status monitoring, audit trails, and basic reports for intuitive troubleshooting.
* **🔔 Health Alert Mechanism**: Supports custom alert rules and event logging to continuously monitor the health status of accounts and requests.
* **💻 Out-of-the-box Web UI**: Comes with a visual management console; perform initialization and daily maintenance directly in your browser after deployment.

---

## 🚀 Quick Start with Docker

> 💡 **Tip**: If you need to use Firecrawl's **asynchronous task callback** feature, please prepare a publicly accessible domain name in advance and configure `WDAPM_WEBHOOK_BASE_URL` in the `.env` file later.

**1. Create directory and download configuration files**

```bash
mkdir -p wdapm && cd wdapm
curl -fsSL -o docker-compose.yml https://raw.githubusercontent.com/Mgrsc/WebDataApiProxyManager/main/docker-compose.yml
curl -fsSL -o .env.example https://raw.githubusercontent.com/Mgrsc/WebDataApiProxyManager/main/.env.example

```

**2. Prepare and modify environment variables**

```bash
cp .env.example .env

```

Open the `.env` file and modify it as needed. Generally, you only need to focus on `WDAPM_WEBHOOK_BASE_URL` (only used for Firecrawl async webhook scenarios; enter your public access address).

**3. Pull images and start the service**

```bash
docker compose pull
docker compose up -d

```

**4. Access the Console**

Once the service is started, access it via your browser:

```text
http://<Your-Server-IP>:8080

```

---

## 🛠️ Initialization & Best Practices

When you open the console for the first time, the system will guide you to **set an administrator password** and automatically generate a **default Platform API Key**. Please save this key securely. We recommend completing the basic configuration in the following order:

1. **Add Upstream Accounts**: Enter your account information for Exa, Tavily, Firecrawl, Jina, etc.
2. **Bind Egress Proxies**: Configure Egress Proxies for accounts that require them.
3. **Set Alert Rules**: Configure monitoring alerts to detect anomalies promptly.

> ⚠️ **Important Note on IP Risk Control**
> If no egress proxy is bound to a Provider account, WDAPM will request the upstream directly using the host IP. In scenarios involving multiple accounts sharing a single IP, high-frequency calls, or cross-regional use, it is highly likely to trigger upstream risk control policies (e.g., rate limiting, CAPTCHAs, account flagging, or bans).
> **Strongly Recommended:** When managing a large number of accounts or requiring high stability, ensure you configure **independent Egress Proxies** for each account to maintain IP isolation.

---

## 🎯 Integration

Simply replace the original upstream Base URL with the WDAPM service address and include your **Platform API Key** in the request headers to access the corresponding paths:

* Proxy Exa: `/exa/...`
* Proxy Tavily: `/tavily/...`
* Proxy Firecrawl: `/firecrawl/...`
* Proxy Jina Reader: `/jina/r/...`
* Proxy Jina Search: `/jina/s/...`

### Jina Route Mapping

Jina is handled slightly differently from providers such as Exa or Tavily. WDAPM maps Jina's upstream host prefix into the route path:

* `https://r.jina.ai/...` -> `/jina/r/...`
* `https://s.jina.ai/...` -> `/jina/s/...`

For example:

```text
Original upstream:
https://r.jina.ai/http://example.com

Via WDAPM:
http://<Your-Server-IP>:8080/jina/r/http://example.com
```

```text
Original upstream:
https://s.jina.ai/http://example.com

Via WDAPM:
http://<Your-Server-IP>:8080/jina/s/http://example.com
```

> Note: Jina routes must start with `r/` or `s/`. Requests like `/jina/...` without that prefix are invalid.

---

## ⚡ Concurrency & Performance

The following data is based on a local test environment (single-machine deployment + SQLite + Mock upstream) and is provided for **capacity planning reference** only, not as a strict SLA commitment:

* **Recommended Operating Range**: Ideal concurrency for stable continuous operation is approximately **192 - 256**.
* **Estimated Throughput**: At recommended concurrency, it can process roughly **12,000 - 18,000 req/min**.
* **Saturation Limit**: The system enters a significant saturation zone when total concurrency reaches `384` (sustainable for 30 minutes, but not recommended for regular loads).

---

## 📄 License

This project is licensed under the [Apache-2.0](./LICENSE) License.

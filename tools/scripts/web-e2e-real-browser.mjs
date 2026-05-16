#!/usr/bin/env node

import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const ROOT_DIR = path.resolve(SCRIPT_DIR, "../..");

const env = process.env;
const config = {
  browserUrl: env.RESEARCH_BROWSER_URL || "http://localhost:9222",
  webUrl: env.LITTER_WEB_URL || "http://127.0.0.1:8080/",
  transport: chooseTransport(env.LITTER_WEB_TRANSPORT, env.LITTER_ALLEYCAT_PAIR_PAYLOAD),
  websocketUrl: env.LITTER_CODEX_WS_URL || "ws://127.0.0.1:8390",
  alleycatPairPayload: (env.LITTER_ALLEYCAT_PAIR_PAYLOAD || "").trim(),
  alleycatAgent: env.LITTER_ALLEYCAT_AGENT || "codex",
  timeoutMs: positiveInt(env.LITTER_WEB_E2E_TIMEOUT_MS, 30_000),
  keepTab: env.LITTER_WEB_E2E_KEEP_TAB === "1",
  sendTurn: env.LITTER_WEB_E2E_SEND_TURN === "1",
  message: env.LITTER_WEB_E2E_MESSAGE || "E2E smoke test from Litter Web.",
  viewportWidth: positiveInt(env.LITTER_WEB_E2E_VIEWPORT_WIDTH, 1440),
  viewportHeight: positiveInt(env.LITTER_WEB_E2E_VIEWPORT_HEIGHT, 1000),
  artifactRoot: env.LITTER_WEB_E2E_ARTIFACT_DIR
    ? path.resolve(env.LITTER_WEB_E2E_ARTIFACT_DIR)
    : path.join(ROOT_DIR, "artifacts/web-e2e"),
};

const runStamp = new Date().toISOString().replace(/[:.]/g, "-");
const runDir = path.join(config.artifactRoot, runStamp);

const summary = {
  ok: false,
  startedAt: new Date().toISOString(),
  browserUrl: config.browserUrl,
  webUrl: config.webUrl,
  transport: config.transport,
  websocketUrl: config.transport === "websocket" ? config.websocketUrl : undefined,
  alleycat: config.transport === "alleycat"
    ? summarizeAlleycatPairPayload(config.alleycatPairPayload, config.alleycatAgent)
    : undefined,
  artifactDir: runDir,
  steps: [],
  screenshots: [],
  console: [],
  errors: [],
};

class CdpClient {
  constructor(webSocketUrl) {
    this.webSocketUrl = webSocketUrl;
    this.nextId = 1;
    this.pending = new Map();
    this.eventListeners = new Map();
    this.socket = null;
  }

  connect() {
    if (typeof WebSocket !== "function") {
      throw new Error("Node.js WebSocket global is missing. Use Node 22+.");
    }

    this.socket = new WebSocket(this.webSocketUrl);
    this.socket.addEventListener("message", (event) => this.handleMessage(event.data));
    this.socket.addEventListener("close", () => {
      for (const { reject } of this.pending.values()) {
        reject(new Error("CDP socket closed"));
      }
      this.pending.clear();
    });

    return new Promise((resolve, reject) => {
      const timeout = setTimeout(
        () => reject(new Error(`Timed out connecting to ${this.webSocketUrl}`)),
        10_000,
      );

      this.socket.addEventListener(
        "open",
        () => {
          clearTimeout(timeout);
          resolve();
        },
        { once: true },
      );
      this.socket.addEventListener(
        "error",
        () => {
          clearTimeout(timeout);
          reject(new Error(`Failed to connect to ${this.webSocketUrl}`));
        },
        { once: true },
      );
    });
  }

  handleMessage(data) {
    const message = JSON.parse(String(data));
    if (message.id) {
      const pending = this.pending.get(message.id);
      if (!pending) {
        return;
      }
      this.pending.delete(message.id);
      if (message.error) {
        pending.reject(new Error(`${message.error.message}: ${message.error.data || ""}`));
      } else {
        pending.resolve(message.result || {});
      }
      return;
    }

    const listeners = this.eventListeners.get(message.method) || [];
    for (const listener of listeners) {
      listener(message.params || {});
    }
  }

  command(method, params = {}) {
    const id = this.nextId++;
    this.socket.send(JSON.stringify({ id, method, params }));
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
    });
  }

  once(method, predicate = () => true, timeoutMs = config.timeoutMs) {
    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        cleanup();
        reject(new Error(`Timed out waiting for ${method}`));
      }, timeoutMs);

      const listener = (params) => {
        if (!predicate(params)) {
          return;
        }
        cleanup();
        resolve(params);
      };

      const cleanup = () => {
        clearTimeout(timeout);
        const listeners = this.eventListeners.get(method) || [];
        this.eventListeners.set(
          method,
          listeners.filter((candidate) => candidate !== listener),
        );
      };

      const listeners = this.eventListeners.get(method) || [];
      listeners.push(listener);
      this.eventListeners.set(method, listeners);
    });
  }

  on(method, listener) {
    const listeners = this.eventListeners.get(method) || [];
    listeners.push(listener);
    this.eventListeners.set(method, listeners);
  }

  close() {
    if (this.socket) {
      this.socket.close();
    }
  }
}

async function main() {
  await mkdir(runDir, { recursive: true });

  let page = null;
  let target = null;

  try {
    target = await createTab(config.browserUrl);
    page = new CdpClient(target.webSocketDebuggerUrl);
    await page.connect();

    await page.command("Runtime.enable");
    await page.command("Page.enable");
    await page.command("Network.enable");
    await page.command("Network.setBypassServiceWorker", { bypass: true });
    await page.command("Network.clearBrowserCache");
    page.on("Runtime.consoleAPICalled", (params) => {
      summary.console.push({
        type: params.type,
        text: (params.args || []).map((arg) => arg.value ?? arg.description ?? "").join(" "),
      });
    });
    page.on("Runtime.exceptionThrown", (params) => {
      summary.console.push({
        type: "exception",
        text:
          params.exceptionDetails?.exception?.description ||
          params.exceptionDetails?.text ||
          "Runtime exception",
      });
    });
    await page.command("Emulation.setDeviceMetricsOverride", {
      width: config.viewportWidth,
      height: config.viewportHeight,
      deviceScaleFactor: 1,
      mobile: false,
    });
    await page.command("Page.bringToFront");

    await navigate(page, config.webUrl);
    await waitForCondition(
      page,
      "attach choice",
      `
        const choice = document.querySelector("#attach-choice");
        const alleycat = document.querySelector("#choose-alleycat");
        const websocket = document.querySelector("#choose-websocket");
        return {
          ok: Boolean(choice && alleycat && websocket && window.__litterWebReady === true),
          title: document.title,
          attachMode: document.body.dataset.attachMode || "",
        };
      `,
    );
    await captureScreenshot(page, "01_WebHome");

    await connectApp(page);

    await waitForCondition(
      page,
      "connected status",
      `
        const status = document.querySelector("#connection-status")?.textContent.trim() || "";
        const error = visibleError();
        return { ok: status.toLowerCase() === "connected" && !error, status, error };
      `,
    );
    await captureScreenshot(page, "02_WebConnected");

    const threads = await waitForCondition(
      page,
      "thread list",
      `
        const buttons = Array.from(document.querySelectorAll("#thread-list .thread"));
        const error = visibleError();
        return {
          ok: buttons.length > 0 && !error,
          count: buttons.length,
          firstThread: buttons[0]?.textContent.trim() || null,
          error,
        };
      `,
    );

    await evaluate(
      page,
      `
        (() => {
          document.querySelector("#thread-list .thread")?.click();
          return true;
        })()
      `,
    );

    const conversation = await waitForCondition(
      page,
      "conversation",
      `
        const title = document.querySelector("#thread-title")?.textContent.trim() || "";
        const meta = document.querySelector("#thread-meta")?.textContent.trim() || "";
        const error = visibleError();
        return {
          ok: Boolean(title) && title !== "No thread selected" && !error,
          title,
          meta,
          itemCount: document.querySelectorAll("#conversation-items .message").length,
          error,
        };
      `,
    );
    await captureScreenshot(page, "03_WebConversation");

    if (config.sendTurn) {
      await sendTurn(page);
      await captureScreenshot(page, "04_WebSendTurn");
    }

    const dom = await collectDomSummary(page);
    Object.assign(summary, {
      ok: true,
      threadCount: threads.count,
      firstThread: threads.firstThread,
      selectedThreadTitle: conversation.title,
      selectedThreadMeta: conversation.meta,
      conversationItemCount: conversation.itemCount,
      dom,
    });
  } catch (error) {
    summary.errors.push(error.stack || String(error));
    if (page) {
      try {
        await captureScreenshot(page, "failure");
        summary.dom = await collectDomSummary(page);
      } catch (captureError) {
        summary.errors.push(captureError.stack || String(captureError));
      }
    }
    process.exitCode = 1;
  } finally {
    summary.finishedAt = new Date().toISOString();
    await writeFile(path.join(runDir, "summary.json"), `${JSON.stringify(summary, null, 2)}\n`);

    if (page && !config.keepTab) {
      try {
        await page.command("Page.close");
      } catch {
        // Tab may already be closed after a browser-side failure.
      }
    }
    if (page) {
      page.close();
    }
  }

  if (summary.ok) {
    console.log(`Web E2E passed: ${runDir}`);
  } else {
    console.error(`Web E2E failed: ${runDir}`);
  }
}

async function createTab(browserUrl) {
  const direct = await tryCreateTabWithHttp(browserUrl);
  if (direct) {
    return direct;
  }

  const version = await fetchJson(browserEndpoint(browserUrl, "/json/version"));
  if (!version.webSocketDebuggerUrl) {
    throw new Error(`CDP browser endpoint missing webSocketDebuggerUrl at ${browserUrl}`);
  }

  const browser = new CdpClient(version.webSocketDebuggerUrl);
  await browser.connect();
  try {
    const result = await browser.command("Target.createTarget", { url: "about:blank" });
    const targets = await fetchJson(browserEndpoint(browserUrl, "/json/list"));
    const target = targets.find((candidate) => candidate.id === result.targetId);
    if (!target?.webSocketDebuggerUrl) {
      throw new Error(`Created target ${result.targetId}, but page WebSocket was not listed`);
    }
    return target;
  } finally {
    browser.close();
  }
}

async function tryCreateTabWithHttp(browserUrl) {
  const endpoint = browserEndpoint(browserUrl, "/json/new");
  endpoint.search = "about:blank";

  for (const method of ["PUT", "GET"]) {
    const response = await fetch(endpoint, { method }).catch(() => null);
    if (!response?.ok) {
      continue;
    }
    const target = await response.json();
    if (target.webSocketDebuggerUrl) {
      return target;
    }
  }

  return null;
}

async function navigate(page, url) {
  const load = page.once("Page.loadEventFired", () => true, config.timeoutMs).catch(() => null);
  await page.command("Page.navigate", { url });
  await load;
}

async function sendTurn(page) {
  await evaluate(
    page,
    `
      (() => {
        const input = document.querySelector("#composer-input");
        const form = document.querySelector("#composer-form");
        input.value = ${JSON.stringify(config.message)};
        input.dispatchEvent(new Event("input", { bubbles: true }));
        input.dispatchEvent(new Event("change", { bubbles: true }));
        form.requestSubmit();
        return true;
      })()
    `,
  );

  await sleep(1_000);
  const result = await evaluate(page, `(() => ({ error: visibleError() }))()`);
  if (result.error) {
    throw new Error(`Send-turn path showed error: ${result.error}`);
  }
}

async function connectApp(page) {
  if (config.transport === "alleycat") {
    if (!config.alleycatPairPayload) {
      throw new Error("LITTER_ALLEYCAT_PAIR_PAYLOAD is required for Alleycat E2E mode");
    }
    await evaluate(
      page,
      `
        (() => {
          document.querySelector("#choose-alleycat")?.click();
          const payload = document.querySelector("#alleycat-pair-payload");
          const agent = document.querySelector("#alleycat-agent");
          const form = document.querySelector("#alleycat-form");
          payload.value = ${JSON.stringify(config.alleycatPairPayload)};
          payload.dispatchEvent(new Event("input", { bubbles: true }));
          payload.dispatchEvent(new Event("change", { bubbles: true }));
          agent.value = ${JSON.stringify(config.alleycatAgent)};
          agent.dispatchEvent(new Event("input", { bubbles: true }));
          agent.dispatchEvent(new Event("change", { bubbles: true }));
          form.requestSubmit();
          return true;
        })()
      `,
    );
    return;
  }

  await evaluate(
    page,
    `
      (() => {
        document.querySelector("#choose-websocket")?.click();
        const input = document.querySelector("#server-url");
        const form = document.querySelector("#websocket-form");
        input.value = ${JSON.stringify(config.websocketUrl)};
        input.dispatchEvent(new Event("input", { bubbles: true }));
        input.dispatchEvent(new Event("change", { bubbles: true }));
        form.requestSubmit();
        return true;
      })()
    `,
  );
}

async function waitForCondition(page, label, body, timeoutMs = config.timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  let last = null;
  let lastError = null;

  while (Date.now() < deadline) {
    try {
      last = await evaluate(page, conditionExpression(body));
      if (last?.ok) {
        summary.steps.push({ label, ok: true, details: last });
        return last;
      }
    } catch (error) {
      lastError = error;
    }
    await sleep(250);
  }

  const details = lastError ? lastError.message : JSON.stringify(last);
  throw new Error(`Timed out waiting for ${label}: ${details}`);
}

function conditionExpression(body) {
  return `
    (() => {
      const visibleError = () => {
        const banner = document.querySelector("#error-banner");
        if (!banner || banner.hidden) return "";
        return banner.textContent.trim();
      };
      ${body}
    })()
  `;
}

async function collectDomSummary(page) {
  return evaluate(
    page,
    `
      (() => {
        const text = (selector) => document.querySelector(selector)?.textContent.trim() || "";
        const errorBanner = document.querySelector("#error-banner");
        return {
          title: document.title,
          location: location.href,
          attachMode: document.body.dataset.attachMode || "",
          alleycatAgent: document.querySelector("#alleycat-agent")?.value || "",
          connectionStatus: text("#connection-status"),
          threadCount: document.querySelectorAll("#thread-list .thread").length,
          threadTitle: text("#thread-title"),
          threadMeta: text("#thread-meta"),
          conversationItemCount: document.querySelectorAll("#conversation-items .message").length,
          errorVisible: Boolean(errorBanner && !errorBanner.hidden),
          error: text("#error-banner"),
        };
      })()
    `,
  );
}

async function captureScreenshot(page, name) {
  const result = await page.command("Page.captureScreenshot", {
    format: "png",
    captureBeyondViewport: false,
  });
  const filename = `${name}.png`;
  const outPath = path.join(runDir, filename);
  await writeFile(outPath, Buffer.from(result.data, "base64"));
  summary.screenshots.push(filename);
}

async function evaluate(page, expression) {
  const result = await page.command("Runtime.evaluate", {
    expression,
    awaitPromise: true,
    returnByValue: true,
    userGesture: true,
  });

  if (result.exceptionDetails) {
    const description =
      result.exceptionDetails.exception?.description ||
      result.exceptionDetails.text ||
      "Runtime.evaluate failed";
    throw new Error(description);
  }

  return result.result?.value;
}

async function fetchJson(url) {
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`GET ${url} failed with HTTP ${response.status}`);
  }
  return response.json();
}

function browserEndpoint(browserUrl, pathname) {
  const url = new URL(browserUrl);
  const basePath = url.pathname.endsWith("/") ? url.pathname.slice(0, -1) : url.pathname;
  url.pathname = `${basePath}${pathname}`.replace(/\/{2,}/g, "/");
  url.search = "";
  return url;
}

function positiveInt(value, fallback) {
  const parsed = Number.parseInt(value || "", 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : fallback;
}

function chooseTransport(value, pairPayload) {
  const requested = (value || "").trim().toLowerCase();
  if (requested === "alleycat" || requested === "websocket") {
    return requested;
  }
  return (pairPayload || "").trim() ? "alleycat" : "websocket";
}

function summarizeAlleycatPairPayload(pairPayload, agent) {
  if (!pairPayload) {
    return { configured: false, agent };
  }
  try {
    const payload = JSON.parse(pairPayload);
    return {
      configured: true,
      agent,
      v: payload.v,
      nodeId: payload.node_id || null,
      relay: payload.relay || null,
      token: payload.token ? "<redacted>" : null,
    };
  } catch {
    return { configured: true, agent, parseError: true, token: "<redacted>" };
  }
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

await main();

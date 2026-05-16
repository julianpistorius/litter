import init, { WasmAlleycatConnection, WasmWebClient } from "./pkg/codex_web_client.js";

let client;
let socket;
let alleycatConnection;
let activeTransport = "none";
let snapshot = {};

const elements = {
  attachChoice: document.querySelector("#attach-choice"),
  chooseAlleycat: document.querySelector("#choose-alleycat"),
  chooseWebsocket: document.querySelector("#choose-websocket"),
  alleycatForm: document.querySelector("#alleycat-form"),
  websocketForm: document.querySelector("#websocket-form"),
  attachBackButtons: document.querySelectorAll("[data-attach-back]"),
  serverUrl: document.querySelector("#server-url"),
  alleycatPairPayload: document.querySelector("#alleycat-pair-payload"),
  alleycatAgent: document.querySelector("#alleycat-agent"),
  status: document.querySelector("#connection-status"),
  refreshThreads: document.querySelector("#refresh-threads"),
  threadList: document.querySelector("#thread-list"),
  threadTitle: document.querySelector("#thread-title"),
  threadMeta: document.querySelector("#thread-meta"),
  items: document.querySelector("#conversation-items"),
  composerForm: document.querySelector("#composer-form"),
  composerInput: document.querySelector("#composer-input"),
  errorBanner: document.querySelector("#error-banner"),
};

async function main() {
  await init();
  client = new WasmWebClient();

  const saved = window.localStorage.getItem("litter.web.serverUrl");
  if (saved) {
    elements.serverUrl.value = saved;
  }
  elements.alleycatPairPayload.value =
    window.localStorage.getItem("litter.web.alleycatPairPayload") || "";
  elements.alleycatAgent.value = window.localStorage.getItem("litter.web.alleycatAgent") || "codex";
  showAttachChoice();

  elements.chooseAlleycat.addEventListener("click", () => {
    showAttachForm("alleycat");
  });

  elements.chooseWebsocket.addEventListener("click", () => {
    showAttachForm("websocket");
  });

  for (const button of elements.attachBackButtons) {
    button.addEventListener("click", () => {
      showAttachChoice();
    });
  }

  elements.alleycatForm.addEventListener("submit", (event) => {
    event.preventDefault();
    dispatch({
      type: "configureAlleycat",
      pairPayload: elements.alleycatPairPayload.value,
      agent: elements.alleycatAgent.value,
    });
  });

  elements.websocketForm.addEventListener("submit", (event) => {
    event.preventDefault();
    dispatch({ type: "configureServer", url: elements.serverUrl.value });
  });

  elements.refreshThreads.addEventListener("click", () => {
    dispatch({ type: "requestThreads" });
  });

  elements.composerForm.addEventListener("submit", (event) => {
    event.preventDefault();
    const threadId = snapshot.activeThreadId;
    const text = elements.composerInput.value;
    if (!threadId) {
      setLocalError("Select a thread before sending.");
      return;
    }
    dispatch({ type: "sendTurn", threadId, text });
    if (text.trim()) {
      elements.composerInput.value = "";
    }
  });

  window.addEventListener("beforeinstallprompt", (event) => {
    event.preventDefault();
  });

  if ("serviceWorker" in navigator) {
    let refreshing = false;
    navigator.serviceWorker.addEventListener("controllerchange", () => {
      if (refreshing) {
        return;
      }
      refreshing = true;
      window.location.reload();
    });
    navigator.serviceWorker.register("./sw.js").then((registration) => {
      registration.update().catch((error) => {
        console.warn("service worker update failed", error);
      });
    }).catch((error) => {
      console.warn("service worker registration failed", error);
    });
  }

  render(JSON.parse(client.snapshotJson()));
  window.__litterWebReady = true;
}

function dispatch(event) {
  clearLocalError();
  let result;
  try {
    result = JSON.parse(client.dispatchEventJson(JSON.stringify(event)));
  } catch (error) {
    setLocalError(errorMessage(error));
    return;
  }
  render(result.snapshot);
  for (const effect of result.effects) {
    executeEffect(effect);
  }
}

function executeEffect(effect) {
  switch (effect.type) {
    case "openWebSocket":
      openWebSocket(effect.url);
      break;
    case "openAlleycat":
      openAlleycat(effect.config);
      break;
    case "sendRpc":
      sendRpc(effect.message);
      break;
    case "persistConfig":
      window.localStorage.setItem("litter.web.transportMode", "websocket");
      window.localStorage.setItem("litter.web.serverUrl", effect.config.websocketUrl);
      break;
    case "persistAlleycatConfig":
      window.localStorage.setItem("litter.web.transportMode", "alleycat");
      window.localStorage.setItem("litter.web.alleycatPairPayload", effect.config.pairPayload);
      window.localStorage.setItem("litter.web.alleycatAgent", effect.config.agent);
      break;
    default:
      console.warn("unknown effect", effect);
  }
}

function openWebSocket(url) {
  closeTransport();

  activeTransport = "websocket";
  socket = new WebSocket(url);
  socket.addEventListener("open", () => dispatch({ type: "webSocketOpened" }));
  socket.addEventListener("message", (event) => {
    dispatch({ type: "webSocketMessage", text: String(event.data) });
  });
  socket.addEventListener("close", (event) => {
    dispatch({
      type: "webSocketClosed",
      reason: event.reason || `Closed (${event.code})`,
    });
  });
  socket.addEventListener("error", () => {
    setLocalError("WebSocket error. Check the server URL and proxy/auth setup.");
  });
}

async function openAlleycat(config) {
  closeTransport();

  activeTransport = "alleycat";
  try {
    const connection = await WasmAlleycatConnection.connect(config.pairPayload, config.agent);
    if (activeTransport !== "alleycat") {
      connection.close();
      return;
    }
    alleycatConnection = connection;
    dispatch({ type: "webSocketOpened" });
    readAlleycatLoop(connection);
  } catch (error) {
    dispatch({ type: "webSocketClosed", reason: "Alleycat connection failed" });
    setLocalError(`Alleycat connection failed. ${error}`);
  }
}

async function readAlleycatLoop(connection) {
  let readError = "";
  try {
    while (activeTransport === "alleycat" && alleycatConnection === connection) {
      const text = await connection.nextMessageJson();
      if (text === null || text === undefined) {
        break;
      }
      dispatch({ type: "webSocketMessage", text: String(text) });
    }
  } catch (error) {
    if (activeTransport === "alleycat" && alleycatConnection === connection) {
      readError = `Alleycat read failed. ${error}`;
    }
  } finally {
    if (activeTransport === "alleycat" && alleycatConnection === connection) {
      dispatch({ type: "webSocketClosed", reason: "Alleycat disconnected" });
      if (readError) {
        setLocalError(readError);
      }
    }
  }
}

function sendRpc(message) {
  if (activeTransport === "alleycat" && alleycatConnection) {
    alleycatConnection.sendJson(JSON.stringify(message)).catch((error) => {
      setLocalError(`Alleycat write failed. ${error}`);
    });
    return;
  }

  if (activeTransport !== "websocket" || !socket || socket.readyState !== WebSocket.OPEN) {
    setLocalError("WebSocket is not connected.");
    return;
  }
  socket.send(JSON.stringify(message));
}

function closeTransport() {
  if (socket) {
    socket.close();
    socket = undefined;
  }
  if (alleycatConnection) {
    alleycatConnection.close();
    alleycatConnection = undefined;
  }
  activeTransport = "none";
}

function render(nextSnapshot) {
  snapshot = nextSnapshot;
  const connection = snapshot.connection || {};
  elements.status.textContent = connection.message || connection.status || "Disconnected";

  renderError(snapshot.lastError);
  renderThreads(snapshot.threads || []);
  renderConversation(snapshot.activeThread);
}

function renderThreads(threads) {
  elements.threadList.replaceChildren(
    ...threads.map((thread) => {
      const row = document.createElement("li");
      const button = document.createElement("button");
      button.type = "button";
      button.className = thread.id === snapshot.activeThreadId ? "thread active" : "thread";
      button.addEventListener("click", () => {
        dispatch({ type: "selectThread", threadId: thread.id });
      });

      const title = document.createElement("span");
      title.className = "thread-title";
      title.textContent = thread.title || "Untitled thread";

      const meta = document.createElement("span");
      meta.className = "thread-meta";
      meta.textContent = [thread.status, thread.cwd].filter(Boolean).join(" - ");

      button.append(title, meta);
      row.append(button);
      return row;
    }),
  );
}

function renderConversation(thread) {
  if (!thread) {
    elements.threadTitle.textContent = "No thread selected";
    elements.threadMeta.textContent = "Connect, then choose a thread.";
    elements.items.replaceChildren(emptyState("No conversation loaded."));
    return;
  }

  elements.threadTitle.textContent = thread.summary.title || "Untitled thread";
  elements.threadMeta.textContent = [thread.summary.status, thread.summary.cwd]
    .filter(Boolean)
    .join(" - ");

  const nodes = (thread.items || []).map((item) => {
    const block = document.createElement("article");
    block.className = `message ${item.kind}`;

    const kind = document.createElement("div");
    kind.className = "message-kind";
    kind.textContent = item.kind;

    const text = document.createElement("pre");
    text.textContent = item.text || "";

    block.append(kind, text);
    return block;
  });

  elements.items.replaceChildren(...(nodes.length ? nodes : [emptyState("No turns loaded.")]));
}

function renderError(message) {
  if (!message) {
    elements.errorBanner.hidden = true;
    elements.errorBanner.textContent = "";
    return;
  }
  elements.errorBanner.hidden = false;
  elements.errorBanner.textContent = message;
}

function setLocalError(message) {
  elements.errorBanner.hidden = false;
  elements.errorBanner.textContent = message;
}

function clearLocalError() {
  elements.errorBanner.hidden = true;
  elements.errorBanner.textContent = "";
}

function errorMessage(error) {
  if (error instanceof Error && error.message) {
    return error.message;
  }
  if (typeof error === "string") {
    return error;
  }
  return String(error);
}

function showAttachChoice() {
  elements.attachChoice.hidden = false;
  elements.alleycatForm.hidden = true;
  elements.websocketForm.hidden = true;
  document.body.dataset.attachMode = "choice";
  clearLocalError();
}

function showAttachForm(mode) {
  elements.attachChoice.hidden = true;
  elements.alleycatForm.hidden = mode !== "alleycat";
  elements.websocketForm.hidden = mode !== "websocket";
  document.body.dataset.attachMode = mode;
  clearLocalError();
  if (mode === "alleycat") {
    elements.alleycatPairPayload.focus();
  } else {
    elements.serverUrl.focus();
  }
}

function emptyState(message) {
  const node = document.createElement("p");
  node.className = "empty-state";
  node.textContent = message;
  return node;
}

main().catch((error) => {
  console.error(error);
  setLocalError(String(error));
});

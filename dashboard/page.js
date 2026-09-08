(() => {
  const startedAt = Date.now();
  const clock = document.querySelector("#clock");
  const dateLine = document.querySelector("#date-line");
  const tickEl = document.querySelector("#tick");
  const phaseEl = document.querySelector("#phase");
  const phaseLabel = document.querySelector("#phase-label");
  const updated = document.querySelector("#updated");
  const uptime = document.querySelector("#uptime");
  const kindleApi = document.querySelector("#kindle-api");
  const banner = document.querySelector("#banner");
  const bars = document.querySelector("#bars");

  const messages = [
    "If this text and the black/white blocks change every few seconds, Servo JS + SWGL + FBInk auto-refresh is working on device.",
    "Tick advanced. Compare the DOM tick number against the previous frame.",
    "Phase inverted. The large box should flip between white-on-black and black-on-white.",
    "Bars moved. At least one solid block should have shifted position.",
    "Clock updated. Hours, minutes, and seconds should all be readable on e-ink.",
  ];

  for (let i = 0; i < 16; i += 1) {
    const bar = document.createElement("div");
    bar.className = "bar";
    bar.dataset.index = String(i);
    bars.appendChild(bar);
  }

  let tick = 0;
  let apiProbeDone = false;

  function pad(value) {
    return String(value).padStart(2, "0");
  }

  function formatClock(date) {
    return `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
  }

  function formatDate(date) {
    return date.toLocaleDateString(undefined, {
      weekday: "short",
      year: "numeric",
      month: "short",
      day: "numeric",
    });
  }

  function formatUptime(ms) {
    const totalSeconds = Math.floor(ms / 1000);
    const hours = Math.floor(totalSeconds / 3600);
    const minutes = Math.floor((totalSeconds % 3600) / 60);
    const seconds = totalSeconds % 60;
    if (hours > 0) {
      return `${hours}h ${pad(minutes)}m ${pad(seconds)}s`;
    }
    if (minutes > 0) {
      return `${minutes}m ${pad(seconds)}s`;
    }
    return `${seconds}s`;
  }

  function inspectKindleApi() {
    const kindle = navigator.kindle;
    if (!kindle || !kindle.screen) {
      kindleApi.textContent = "missing";
      return;
    }
    const screen = kindle.screen;
    const width = screen.width ?? "?";
    const height = screen.height ?? "?";
    const canRefresh = typeof screen.refreshNow === "function";
    const canBattery = typeof kindle.device?.battery === "function";
    kindleApi.textContent = canRefresh
      ? `${width}x${height} refreshNow${canBattery ? "+device" : ""}`
      : `${width}x${height} attrs only`;
  }

  async function probeKindleApis() {
    if (apiProbeDone) {
      return;
    }
    apiProbeDone = true;
    const screen = navigator.kindle?.screen;
    if (!screen || typeof screen.refreshNow !== "function") {
      return;
    }
    try {
      await screen.refreshNow({
        waveform: "quality",
        elements: [phaseEl, banner],
      });
      const last = screen.lastRefresh?.();
      if (last != null) {
        banner.textContent = `refreshNow ok; lastRefresh=${last}`;
      }
    } catch (error) {
      banner.textContent = `refreshNow error: ${error && error.message ? error.message : error}`;
    }
    try {
      const battery = await navigator.kindle.device.battery();
      kindleApi.textContent += ` bat=${Math.round(battery.percentage)}%`;
    } catch (_) {
      kindleApi.textContent += " bat=n/a";
    }
  }

  function paintBars(step) {
    const nodes = bars.querySelectorAll(".bar");
    nodes.forEach((node, index) => {
      const active = (index + step) % 4 === 0 || index === step % nodes.length;
      node.classList.toggle("on", active);
    });
  }

  function mutate() {
    const now = new Date();
    tick += 1;
    const phase = tick % 2;

    clock.textContent = formatClock(now);
    dateLine.textContent = formatDate(now);
    tickEl.textContent = String(tick);
    phaseEl.dataset.phase = String(phase);
    phaseLabel.textContent = phase === 0 ? "light panel" : "dark panel";
    updated.textContent = formatClock(now);
    uptime.textContent = formatUptime(Date.now() - startedAt);
    if (tick > 1) {
      banner.textContent = messages[tick % messages.length];
    }
    banner.classList.toggle("alt", phase === 1);
    paintBars(tick);
    inspectKindleApi();

    if (tick === 3) {
      probeKindleApis();
    }
  }

  inspectKindleApi();
  mutate();
  setInterval(mutate, 1000);
})();

(() => {
  const runtime = document.querySelector("#runtime");
  const updated = document.querySelector("#updated");

  function stamp(label) {
    if (runtime) {
      runtime.textContent = label;
    }
    if (updated) {
      updated.textContent = new Date().toLocaleTimeString();
    }
  }

  window.addEventListener("ped:update", (event) => {
    const detail = event.detail || {};
    const label = detail.runtime || detail.type || detail.message || "event";
    stamp(String(label));
  });

  stamp("running");
})();

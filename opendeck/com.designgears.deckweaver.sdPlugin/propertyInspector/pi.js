const ICON_PICKER_HTML = `
<section class="section">
  <div class="field-label-row">
    <label class="field-label" for="iconComboboxSearch">Icon</label>
    <button type="button" id="clearIcon" class="btn-link">Clear</button>
  </div>
  <div class="icon-combobox" id="iconCombobox">
    <button type="button" class="icon-combobox-trigger" id="iconComboboxTrigger" aria-haspopup="listbox" aria-expanded="false">
      <span class="icon-combobox-preview" id="iconComboboxPreview" aria-hidden="true"></span>
      <span class="icon-combobox-text" id="iconComboboxText">Choose an icon…</span>
    </button>
    <div class="icon-combobox-panel" id="iconComboboxPanel" hidden>
      <input type="search" id="iconComboboxSearch" placeholder="Search icons…" autocomplete="off" />
      <ul class="icon-combobox-list" id="iconComboboxList" role="listbox"></ul>
    </div>
  </div>
</section>
`;

const ICON_LIST_LIMIT = 120;

let websocket = null;
let uuid = null;
let actionInfo = null;
let settings = {};
let faIcons = [];
let faIconsLoaded = false;
let iconComboboxOpen = false;

function connectElgatoStreamDeckSocket(inPort, inUUID, inRegisterEvent, inInfo, inActionInfo) {
  uuid = inUUID;
  actionInfo = JSON.parse(inActionInfo || "{}");
  settings = actionInfo.payload?.settings || {};
  websocket = new WebSocket(`ws://127.0.0.1:${inPort}`);
  websocket.onopen = () => {
    websocket.send(JSON.stringify({ event: inRegisterEvent, uuid: inUUID }));
    websocket.send(JSON.stringify({ event: "getSettings", context: inUUID }));
  };
  websocket.onmessage = (evt) => {
    const message = JSON.parse(evt.data);
    if (message.event === "didReceiveSettings") {
      settings = message.payload?.settings || {};
      populateFromSettings();
    }
    if (message.event === "sendToPropertyInspector") {
      handlePluginMessage(message.payload || {});
    }
  };
}

function saveSettings(next) {
  settings = { ...settings, ...next };
  websocket?.send(JSON.stringify({ event: "setSettings", context: uuid, payload: settings }));
}

function setStatus(text, isError = false) {
  const el = document.getElementById("status");
  if (!el) return;
  el.textContent = text;
  el.classList.toggle("error", isError);
}

function populateDeviceSelect(select, devices, selectedId) {
  select.innerHTML = "";
  const empty = document.createElement("option");
  empty.value = "";
  empty.textContent = "Select a device…";
  select.appendChild(empty);
  for (const device of devices) {
    const option = document.createElement("option");
    option.value = device.id;
    option.textContent = `${device.name} (${device.id})`;
    option.dataset.deviceType = device.device_type === "Source" ? "source" : "target";
    if (device.id === selectedId) option.selected = true;
    select.appendChild(option);
  }
}

function mountIconPicker() {
  const mount = document.getElementById("iconPickerMount");
  if (!mount) return;
  mount.innerHTML = ICON_PICKER_HTML;
  updateClearIconButton();
}

function whiteIconNode(svg, size) {
  const wrap = document.createElement("span");
  if (!svg) return wrap;
  wrap.innerHTML = svg.replace(/currentColor/g, "#ffffff");
  const svgEl = wrap.querySelector("svg");
  if (svgEl) {
    svgEl.setAttribute("width", String(size));
    svgEl.setAttribute("height", String(size));
  }
  return wrap;
}

function iconBySlug(slug) {
  return faIcons.find((icon) => icon.slug === slug) || null;
}

function iconSearchText(icon) {
  return `${icon.label} ${icon.slug}`.toLowerCase();
}

function filteredIcons(query) {
  const normalized = query.trim().toLowerCase();
  const matches = normalized
    ? faIcons.filter((icon) => iconSearchText(icon).includes(normalized))
    : faIcons;
  return matches.slice(0, ICON_LIST_LIMIT);
}

function setSelectedIconDisplay(slug) {
  const preview = document.getElementById("iconComboboxPreview");
  const text = document.getElementById("iconComboboxText");
  if (!preview || !text) return;

  preview.replaceChildren();
  if (!slug) {
    text.textContent = "Choose an icon…";
    updateClearIconButton();
    return;
  }

  const icon = iconBySlug(slug);
  if (icon?.svg) preview.appendChild(whiteIconNode(icon.svg, 20));
  text.textContent = icon?.label || slug.split("/").pop()?.replace(/-/g, " ") || slug;
  updateClearIconButton();
}

function updateClearIconButton() {
  const clear = document.getElementById("clearIcon");
  if (!clear) return;
  const hasIcon = Boolean(settings.icon_fa);
  clear.disabled = !hasIcon;
}

function selectIcon(icon) {
  if (!icon) return;
  saveSettings({ icon_fa: icon.slug, icon_path: null });
  setSelectedIconDisplay(icon.slug);
  closeIconCombobox();
}

function renderIconComboboxList() {
  const list = document.getElementById("iconComboboxList");
  const search = document.getElementById("iconComboboxSearch");
  if (!list || !search || !faIconsLoaded) return;

  const query = search.value || "";
  const icons = filteredIcons(query);
  list.replaceChildren();

  if (icons.length === 0) {
    const empty = document.createElement("li");
    empty.className = "icon-combobox-empty";
    empty.textContent = query.trim() ? "No matching icons" : "No icons available";
    list.appendChild(empty);
    return;
  }

  const selected = settings.icon_fa || "";
  for (const icon of icons) {
    const item = document.createElement("li");
    item.className = "icon-combobox-option";
    item.role = "option";
    item.dataset.slug = icon.slug;
    if (icon.slug === selected) item.classList.add("active");
    item.appendChild(whiteIconNode(icon.svg, 18));
    const label = document.createElement("span");
    label.textContent = icon.label;
    item.appendChild(label);
    item.addEventListener("mousedown", (event) => {
      event.preventDefault();
      selectIcon(icon);
    });
    list.appendChild(item);
  }

  if (!query.trim() && faIcons.length > icons.length) {
    const hint = document.createElement("li");
    hint.className = "icon-combobox-empty";
    hint.textContent = `Showing ${icons.length} of ${faIcons.length} icons — type to search the full library`;
    list.appendChild(hint);
  }
}

function openIconCombobox() {
  const panel = document.getElementById("iconComboboxPanel");
  const trigger = document.getElementById("iconComboboxTrigger");
  const search = document.getElementById("iconComboboxSearch");
  if (!panel || !trigger || !search) return;
  panel.hidden = false;
  trigger.setAttribute("aria-expanded", "true");
  iconComboboxOpen = true;
  renderIconComboboxList();
  search.value = "";
  search.focus();
}

function closeIconCombobox() {
  const panel = document.getElementById("iconComboboxPanel");
  const trigger = document.getElementById("iconComboboxTrigger");
  if (!panel || !trigger) return;
  panel.hidden = true;
  trigger.setAttribute("aria-expanded", "false");
  iconComboboxOpen = false;
}

async function loadFaIcons() {
  if (faIconsLoaded) return;
  try {
    const response = await fetch("fontawesome-icons.json");
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    faIcons = await response.json();
    faIconsLoaded = true;
  } catch (error) {
    setStatus(`Failed to load Font Awesome icons: ${error.message}`, true);
  }
}

function wireFaIconPicker() {
  mountIconPicker();

  loadFaIcons().then(() => {
    setSelectedIconDisplay(settings.icon_fa || null);
  });

  document.getElementById("iconComboboxTrigger")?.addEventListener("click", () => {
    if (iconComboboxOpen) closeIconCombobox();
    else openIconCombobox();
  });

  document.getElementById("iconComboboxSearch")?.addEventListener("input", renderIconComboboxList);
  document.getElementById("iconComboboxSearch")?.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      event.preventDefault();
      closeIconCombobox();
    }
  });

  document.getElementById("clearIcon")?.addEventListener("click", () => {
    saveSettings({ icon_fa: null, icon_path: null });
    setSelectedIconDisplay(null);
    renderIconComboboxList();
  });

  document.addEventListener("mousedown", (event) => {
    const combobox = document.getElementById("iconCombobox");
    if (!iconComboboxOpen || !combobox) return;
    if (!combobox.contains(event.target)) closeIconCombobox();
  });
}

function initActionInspector(options) {
  window.deckweaverPiOptions = options;
  if (options.showIconPicker !== false) {
    wireFaIconPicker();
  }
  document.getElementById("refresh")?.addEventListener("click", () => {
    websocket?.send(JSON.stringify({ event: "sendToPlugin", context: uuid, payload: { event: "refreshDevices" } }));
  });
  document.getElementById("device")?.addEventListener("change", (event) => {
    const selected = event.target.selectedOptions[0];
    saveSettings({
      device_id: event.target.value || null,
      device_type: selected?.dataset.deviceType || null,
    });
  });
  document.getElementById("volumeStep")?.addEventListener("change", (event) => {
    saveSettings({ volume_step: Number(event.target.value) });
  });
  document.getElementById("orientation")?.addEventListener("change", (event) => {
    saveSettings({ orientation: event.target.value });
  });
  document.getElementById("sourceMix")?.addEventListener("change", (event) => {
    saveSettings({ source_mix: event.target.value });
  });
  document.getElementById("metersEnabled")?.addEventListener("change", (event) => {
    saveSettings({ meters_enabled: event.target.checked });
  });
  document.getElementById("showVolume")?.addEventListener("change", (event) => {
    saveSettings({ show_volume: event.target.checked });
  });
  applyActionVisibility();
}

// action.html is shared by the knob and button actions; only the knob renders the encoder
// strip, so rows that only affect it are hidden elsewhere rather than silently doing nothing.
const KNOB_ACTION_UUID = "com.designgears.deckweaver.knob";

function applyActionVisibility() {
  // Fail open. This first runs from initActionInspector, which the inline script in the page
  // calls before connectElgatoStreamDeckSocket has set actionInfo — so a strict "is it the
  // knob?" test hides the row at load and never brings it back if the settings event that
  // re-runs this doesn't arrive. Hide only once we positively know it is some other action.
  const action = actionInfo?.action;
  const isKnob = !action || action === KNOB_ACTION_UUID;
  document.querySelectorAll("[data-knob-only]").forEach((el) => {
    el.hidden = !isKnob;
  });
}

function populateFromSettings() {
  const options = window.deckweaverPiOptions || {};
  const volumeStep = document.getElementById("volumeStep");
  if (volumeStep) {
    volumeStep.min = String(options.minVolumeStep ?? 1);
    volumeStep.max = String(options.maxVolumeStep ?? 20);
    volumeStep.value = String(settings.volume_step ?? options.defaultVolumeStep ?? 5);
  }
  const sourceMix = document.getElementById("sourceMix");
  if (sourceMix) sourceMix.value = settings.source_mix || "A";
  const orientation = document.getElementById("orientation");
  if (orientation) orientation.value = settings.orientation || "vertical";
  const metersEnabled = document.getElementById("metersEnabled");
  if (metersEnabled) metersEnabled.checked = settings.meters_enabled !== false;
  const showVolume = document.getElementById("showVolume");
  if (showVolume) showVolume.checked = settings.show_volume !== false;
  applyActionVisibility();
  if (options.showIconPicker !== false) {
    setSelectedIconDisplay(settings.icon_fa || null);
    if (iconComboboxOpen) renderIconComboboxList();
  }
}

function handlePluginMessage(payload) {
  if (payload.event !== "devices") return;
  if (!payload.available) {
    setStatus("PipeWeaver is not running on localhost:14565", true);
    return;
  }
  setStatus("Connected to PipeWeaver");

  if (window.deckweaverSwitchMode === true || window.deckweaverSwitchMode === false) {
    handleSwitchDevices(payload);
    return;
  }

  const devices = [...(payload.sources || []), ...(payload.targets || [])];
  const select = document.getElementById("device");
  if (select) populateDeviceSelect(select, devices, settings.device_id);
}

function handleSwitchDevices(payload) {
  const inputMode = window.deckweaverSwitchMode === true;
  const targets = inputMode ? payload.sources || [] : payload.targets || [];
  const hardware = inputMode ? payload.inputHardware || [] : payload.outputHardware || [];
  populateDeviceSelect(document.getElementById("outputDevice"), targets, settings.device_id);
  const physical = document.getElementById("physicalDevice");
  if (!physical) return;
  physical.innerHTML = "";
  const empty = document.createElement("option");
  empty.value = "";
  empty.textContent = "Select a physical device…";
  physical.appendChild(empty);
  for (const device of hardware) {
    const option = document.createElement("option");
    option.value = String(device.node_id ?? "");
    option.dataset.description = device.description || "";
    option.textContent = device.description || device.name || `Node ${device.node_id}`;
    if (device.node_id === settings.hardware_device_node_id) option.selected = true;
    physical.appendChild(option);
  }
}

function initSourceSwitchInspector({ inputMode }) {
  window.deckweaverSwitchMode = inputMode;
  wireFaIconPicker();
  document.getElementById("refresh")?.addEventListener("click", () => {
    websocket?.send(JSON.stringify({ event: "sendToPlugin", context: uuid, payload: { event: "refreshDevices" } }));
  });
  document.getElementById("outputDevice")?.addEventListener("change", (event) => {
    saveSettings({ device_id: event.target.value || null, device_type: inputMode ? "source" : "target" });
  });
  document.getElementById("physicalDevice")?.addEventListener("change", (event) => {
    const selected = event.target.selectedOptions[0];
    saveSettings({
      hardware_device_node_id: selected?.value ? Number(selected.value) : null,
      hardware_device_description: selected?.dataset.description || null,
    });
  });
}

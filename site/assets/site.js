// Copy buttons and the live config -> unit transform. Everything else is static HTML.

(() => {
  const copyButtons = document.querySelectorAll("[data-copy]");

  copyButtons.forEach((button) => {
    button.addEventListener("click", async () => {
      const target = document.querySelector(button.dataset.copy);
      if (!target) return;

      const source =
        target.dataset.copyText !== undefined
          ? target.dataset.copyText
          : target.value !== undefined
            ? target.value
            : target.innerText;
      const text = `${source.replace(/\n+$/, "")}\n`;

      try {
        await navigator.clipboard.writeText(text);
      } catch {
        button.textContent = "Copy failed";
        window.setTimeout(() => {
          button.textContent = "Copy";
        }, 2500);
        return;
      }

      button.dataset.copied = "true";
      button.textContent = "Copied";
      window.clearTimeout(button._copyTimer);
      button._copyTimer = window.setTimeout(() => {
        button.textContent = "Copy";
        delete button.dataset.copied;
      }, 2500);
    });
  });

  const labJson = document.getElementById("lab-json");
  if (!labJson) return;

  const labName = document.getElementById("lab-name");
  const labUnit = document.getElementById("lab-unit");
  const labUnitCode = labUnit.querySelector("code");
  const labUnitName = document.getElementById("lab-unit-name");
  const labStatus = document.getElementById("lab-status");
  const labReset = document.getElementById("lab-reset");
  const labJump = document.getElementById("lab-jump");

  const initialJson = labJson.value;
  const initialName = labName.value;
  const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");

  // Mirror naming.rs: lowercase letters, digits, dashes; 27 characters is the limit once
  // the `appd-` prefix fills the rest of a 32-character Linux user name.
  const sanitizeName = (value) => {
    let name = value
      .toLowerCase()
      .replace(/[^a-z0-9-]+/g, "-")
      .replace(/-{2,}/g, "-")
      .replace(/^-+/, "")
      .slice(0, 27);
    if (name === "all") name = "all-";
    return name;
  };

  // Mirror unit.rs: quote an ExecStart/ExecReload argument the way systemd expects.
  const escapeArg = (value) => {
    const arg = String(value);
    if (arg === "") return '""';

    const needsQuotes = /[\s"'\\]/.test(arg);
    let out = "";
    for (const character of arg) {
      if (character === "%") out += "%%";
      else if (character === "\\" && needsQuotes) out += "\\\\";
      else if (character === '"' && needsQuotes) out += '\\"';
      else out += character;
    }
    return needsQuotes ? `"${out}"` : out;
  };

  const escapeValue = escapeArg;

  const environmentFiles = (value) => {
    if (typeof value === "string") return [value];
    if (Array.isArray(value)) return value.map(String);
    return [];
  };

  const finalizeName = (value) => {
    const name = sanitizeName(value).replace(/-+$/, "");
    return name === "all" ? "all-" : name;
  };

  const parseConfig = (text) => {
    let config;
    try {
      config = JSON.parse(text);
    } catch (error) {
      throw new Error(`JSON: ${error.message}`);
    }
    if (!config || typeof config !== "object" || Array.isArray(config)) {
      throw new Error("config must be a JSON object");
    }
    if (typeof config.exec !== "string" || config.exec === "") {
      throw new Error("`exec` must be a non-empty string");
    }
    return config;
  };

  const unitFor = (app, config) => {
    const permissions =
      config.permissions && typeof config.permissions === "object" ? config.permissions : {};
    const list = (value) => (Array.isArray(value) ? value : []);

    const user =
      typeof config.user === "string" && config.user !== "" ? config.user : `appd-${app}`;
    const group = typeof config.group === "string" && config.group !== "" ? config.group : user;
    const workingDirectory =
      typeof config.working_directory === "string"
        ? config.working_directory
        : `/var/lib/appd-${app}`;
    const description =
      typeof config.description === "string" ? config.description : app;
    const wantedBy = Array.isArray(config.wanted_by) ? config.wanted_by : ["multi-user.target"];
    const args = list(config.args);
    const read = list(permissions.read);
    const write = list(permissions.write);
    const devices = list(permissions.devices);
    const envFiles = environmentFiles(config.environment_file);

    const lines = [];
    const add = (text, kind = "") => lines.push([text, kind]);

    add("[Unit]", "section");
    add(`Description=${description}`);
    add("X-Appd-Managed=1");
    if (permissions.network) add("After=network-online.target");

    add("");
    add("[Service]", "section");
    add("Type=simple");
    add(`User=${user}`);
    add(`Group=${group}`);
    add(`WorkingDirectory=${workingDirectory}`);
    add(`StateDirectory=appd-${app}`);
    add(`ExecStart=${[config.exec, ...args].map(escapeArg).join(" ")}`);

    if (Array.isArray(config.reload) && config.reload.length > 0) {
      add(`ExecReload=${config.reload.map(escapeArg).join(" ")}`, "open");
    }

    add(`Restart=${typeof config.restart === "string" ? config.restart : "on-failure"}`);
    if (config.restart_sec !== undefined && config.restart_sec !== null) {
      add(`RestartSec=${config.restart_sec}`);
    }
    if (typeof config.umask === "string") add(`UMask=${config.umask}`);

    for (const [key, value] of Object.entries(
      config.environment && typeof config.environment === "object" ? config.environment : {},
    )) {
      add(`Environment=${key}=${escapeValue(value)}`);
    }
    for (const file of envFiles) add(`EnvironmentFile=${file}`, "open");

    add("StandardOutput=journal");
    add("StandardError=journal");

    add("");
    add("# hardening defaults", "comment");
    const hardening = [
      "NoNewPrivileges=yes",
      "PrivateTmp=yes",
      "PrivateDevices=yes",
      "ProtectSystem=strict",
      "ProtectHome=yes",
      "ProtectProc=invisible",
      "ProtectClock=yes",
      "ProtectHostname=yes",
      "ProtectKernelTunables=yes",
      "ProtectKernelModules=yes",
      "ProtectKernelLogs=yes",
      "ProtectControlGroups=yes",
      "RestrictNamespaces=yes",
      "RestrictRealtime=yes",
      "RestrictSUIDSGID=yes",
      "LockPersonality=yes",
    ];
    if (!permissions.executable_memory) hardening.push("MemoryDenyWriteExecute=yes");
    hardening.push("SystemCallArchitectures=native");
    for (const line of hardening) add(line);

    add("");
    add("# opened by permissions", "comment");
    add(
      `RestrictAddressFamilies=${
        permissions.network ? "AF_UNIX AF_INET AF_INET6" : "AF_UNIX"
      }`,
      "open",
    );
    if (read.length > 0) add(`ReadOnlyPaths=${read.join(" ")}`, "open");
    if (write.length > 0) add(`ReadWritePaths=${write.join(" ")}`, "open");
    for (const device of devices) add(`DeviceAllow=${device}`, "open");
    if (typeof permissions.apparmor === "string") {
      add(`AppArmorProfile=${permissions.apparmor}`, "open");
    }

    add("");
    add("[Install]", "section");
    add(`WantedBy=${wantedBy.join(" ")}`);

    const opened =
      (permissions.network ? 1 : 0) +
      read.length +
      write.length +
      devices.length +
      (typeof permissions.apparmor === "string" ? 1 : 0) +
      envFiles.length;

    return { lines, hardening: hardening.length, opened };
  };

  const escapeHtml = (value) =>
    String(value).replace(/[&<>]/g, (character) => {
      if (character === "&") return "&amp;";
      if (character === "<") return "&lt;";
      return "&gt;";
    });

  let lastLines = [];
  let lastConfig = null;

  const paint = (lines, flash, scroll) => {
    const seen = new Map();
    for (const [text] of lastLines) seen.set(text, (seen.get(text) || 0) + 1);

    let html = "";
    for (const [text, kind] of lines) {
      let classes = "u-line";
      if (kind) classes += ` u-${kind}`;

      const count = seen.get(text) || 0;
      if (count > 0) {
        seen.set(text, count - 1);
      } else if (flash && lastLines.length > 0) {
        classes += " u-new";
      }

      let body;
      if (kind !== "section" && kind !== "comment" && text.includes("=")) {
        const at = text.indexOf("=");
        body = `<span class="u-key">${escapeHtml(text.slice(0, at))}</span><span class="u-val">${escapeHtml(
          text.slice(at),
        )}</span>`;
      } else {
        body = escapeHtml(text);
      }

      html += `<span class="${classes}">${body}</span>`;
    }

    labUnitCode.innerHTML = html;
    labUnit.dataset.copyText = `${lines.map(([text]) => text).join("\n")}\n`;

    if (scroll) {
      const first = labUnitCode.querySelector(".u-new");
      if (first) {
        const offset = first.offsetTop - labUnit.clientHeight / 2;
        labUnit.scrollTo({
          top: Math.max(0, offset),
          behavior: reducedMotion.matches ? "auto" : "smooth",
        });
      }
    }
  };

  const setStatus = (message, isError = false) => {
    if (labStatus.textContent === message && labStatus.dataset.state === (isError ? "error" : "ok")) {
      return;
    }
    labStatus.textContent = message;
    labStatus.dataset.state = isError ? "error" : "ok";
  };

  const toggles = {
    "lab-network": {
      get: (config) => Boolean(config.permissions && config.permissions.network),
      set: (config, on) => {
        if (on) {
          config.permissions = config.permissions || {};
          config.permissions.network = true;
        } else if (config.permissions) {
          delete config.permissions.network;
        }
      },
    },
    "lab-write": {
      get: (config) =>
        Boolean(
          config.permissions &&
            Array.isArray(config.permissions.write) &&
            config.permissions.write.length > 0,
        ),
      set: (config, on) => {
        if (on) {
          config.permissions = config.permissions || {};
          config.permissions.write = ["/srv/sundial/public"];
        } else if (config.permissions) {
          delete config.permissions.write;
        }
      },
    },
    "lab-jit": {
      get: (config) => Boolean(config.permissions && config.permissions.executable_memory),
      set: (config, on) => {
        if (on) {
          config.permissions = config.permissions || {};
          config.permissions.executable_memory = true;
        } else if (config.permissions) {
          delete config.permissions.executable_memory;
        }
      },
    },
    "lab-reload": {
      get: (config) => Array.isArray(config.reload) && config.reload.length > 0,
      set: (config, on) => {
        if (on) config.reload = ["/bin/kill", "-HUP", "$MAINPID"];
        else delete config.reload;
      },
    },
    "lab-secrets": {
      get: (config) =>
        typeof config.environment_file === "string" ||
        (Array.isArray(config.environment_file) && config.environment_file.length > 0),
      set: (config, on) => {
        if (on) config.environment_file = "/etc/appd/sundial.env";
        else delete config.environment_file;
      },
    },
  };

  const syncToggles = (config) => {
    for (const [id, definition] of Object.entries(toggles)) {
      const input = document.getElementById(id);
      if (input) input.checked = definition.get(config);
    }
  };

  const update = ({ flash = false, scroll = false } = {}) => {
    const sanitized = sanitizeName(labName.value);
    if (labName.value !== "" && sanitized !== labName.value) labName.value = sanitized;
    const app = sanitized || "app";
    labName.style.width = `${Math.max(8, labName.value.length + 1)}ch`;
    labUnitName.textContent = `appd-${app}.service`;

    let config;
    try {
      config = parseConfig(labJson.value);
    } catch (error) {
      labJson.setAttribute("aria-invalid", "true");
      setStatus(error.message, true);
      return;
    }

    labJson.removeAttribute("aria-invalid");
    lastConfig = config;

    const { lines, hardening, opened } = unitFor(app, config);
    const exceptionLabel = opened === 1 ? "exception" : "exceptions";
    setStatus(`${hardening} hardening · ${opened} ${exceptionLabel} · ${lines.length} lines`);
    paint(lines, flash, scroll);
    lastLines = lines;
    syncToggles(config);
  };

  labJson.addEventListener("input", () => update());

  labName.addEventListener("input", () => update({ flash: true }));

  labName.addEventListener("blur", () => {
    const finished = finalizeName(labName.value) || "app";
    if (finished !== labName.value) {
      labName.value = finished;
      update({ flash: true });
    }
  });

  if (labJump) {
    labJump.addEventListener("click", () => {
      const marker = Array.from(labUnitCode.querySelectorAll(".u-comment")).find((line) =>
        line.textContent.includes("opened by permissions"),
      );
      if (!marker) return;

      labUnit.scrollTo({
        top: Math.max(0, marker.offsetTop - 8),
        behavior: reducedMotion.matches ? "auto" : "smooth",
      });
      marker.classList.remove("u-pulse");
      void marker.offsetWidth;
      marker.classList.add("u-pulse");
    });
  }

  labReset.addEventListener("click", () => {
    labJson.value = initialJson;
    labName.value = initialName;
    update({ flash: true, scroll: true });
  });

  for (const [id, definition] of Object.entries(toggles)) {
    const input = document.getElementById(id);
    if (!input) continue;

    input.addEventListener("change", () => {
      let config;
      try {
        config = parseConfig(labJson.value);
      } catch (error) {
        input.checked = !input.checked;
        labJson.setAttribute("aria-invalid", "true");
        setStatus(error.message, true);
        return;
      }

      definition.set(config, input.checked);
      labJson.value = `${JSON.stringify(config, null, 2)}\n`;
      update({ flash: true, scroll: true });
    });
  }

  // The fallback markup in the HTML is the real renderer's output; this first call only
  // normalises it and takes over future edits.
  update();
  if (lastConfig) syncToggles(lastConfig);
})();

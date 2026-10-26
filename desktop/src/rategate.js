







(function (root, factory) {
  if (typeof module !== "undefined" && module.exports) module.exports = factory();
  else root.RateGateUI = factory();
})(typeof self !== "undefined" ? self : this, function () {
  "use strict";

  var STATES = {
    HEALTHY: "healthy",
    APPROACHING: "approaching",
    AT_LIMIT: "at_limit",
    COOLDOWN: "cooldown",
  };

  function scopePct(s) {
    if (!s || !(s.limit > 0)) return 0;
    return (s.used / s.limit) * 100;
  }

  
  function computeState(qs) {
    if (!qs) return STATES.HEALTHY;
    if (qs.cooldown_until && new Date(qs.cooldown_until).getTime() > Date.now())
      return STATES.COOLDOWN;
    var mp = scopePct(qs.minute), dp = scopePct(qs.daily);
    if (mp >= 100 || dp >= 100) return STATES.AT_LIMIT;
    if (mp >= 70 || dp >= 70) return STATES.APPROACHING;
    return STATES.HEALTHY;
  }

  function fmtCountdown(iso) {
    var ms = new Date(iso).getTime() - Date.now();
    if (!(ms > 0)) return "now";
    var s = Math.floor(ms / 1000);
    var h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60);
    if (h > 0) return h + "h" + m + "m";
    if (m > 0) return m + "m" + (s % 60) + "s";
    return s + "s";
  }

  function el(doc, tag, cls, text) {
    var e = doc.createElement(tag);
    if (cls) e.className = cls;
    if (text !== undefined && text !== null) e.textContent = text;
    return e;
  }

  
  function createQuotaMeter(doc, host) {
    var wrap = el(doc, "span", "quota-meter");
    var label = el(doc, "span", "qm-label", "⚡ -/-");
    var bars = el(doc, "span", "qm-bars");
    var minBar = el(doc, "span", "qbar qbar-min");
    var dayBar = el(doc, "span", "qbar qbar-day");
    minBar.appendChild(el(doc, "span", "qfill"));
    dayBar.appendChild(el(doc, "span", "qfill"));
    bars.appendChild(minBar);
    bars.appendChild(dayBar);
    wrap.appendChild(label);
    wrap.appendChild(bars);
    host.appendChild(wrap);

    function setBar(bar, pct, state) {
      var fill = bar.firstChild;
      fill.style.width = Math.min(100, pct) + "%";
      bar.className = "qbar " + (bar === minBar ? "qbar-min " : "qbar-day ") + "qm-" + state;
    }

    return {
      el: wrap,
      update: function (qs) {
        var st = computeState(qs);
        var minute = qs.minute || { used: 0, limit: 0 };
        var daily = qs.daily || { used: 0, limit: 0 };
        var mp = scopePct(minute), dp = scopePct(daily);
        label.textContent =
          "⚡ " + daily.used + "/" + daily.limit + " today · resets in " +
          fmtCountdown(daily.resets_at);
        setBar(minBar, mp, st);
        setBar(dayBar, dp, st);
        wrap.className = "quota-meter qm-" + st;
        wrap.title =
          "minute: " + minute.used + "/" + minute.limit +
          " (resets in " + fmtCountdown(minute.resets_at) + ")\n" +
          "daily: " + daily.used + "/" + daily.limit +
          " (resets in " + fmtCountdown(daily.resets_at) + ")\n" +
          "state: " + st +
          (qs.queue_depth > 0 ? "\nqueued: " + qs.queue_depth : "");
      },
    };
  }

  
  function createFallbackBanner(doc, host) {
    var banner = el(doc, "div", "fallback-banner hidden");
    var msg = el(doc, "span", "fb-msg");
    var dismiss = el(doc, "button", "fb-dismiss", "Dismiss");
    banner.appendChild(msg);
    banner.appendChild(dismiss);
    host.appendChild(banner);
    var dismissedFor = null;

    dismiss.onclick = function () {
      banner.classList.add("hidden");
      dismissedFor = msg.dataset.key || null;
    };

    return {
      el: banner,
      update: function (qs) {
        var fb = qs.fallback;
        if (!fb || !fb.active) { banner.classList.add("hidden"); dismissedFor = null; return; }
        var key = fb.from + ">" + fb.to + "@" + (fb.at || "");
        if (dismissedFor === key) return;
        msg.dataset.key = key;
        var resetTxt = "";
        if (qs.daily && qs.daily.resets_at) resetTxt = " (resets " + fmtCountdown(qs.daily.resets_at) + ")";
        msg.textContent =
          "🔀 Switched to " + fb.to + " - " + fb.from + " " +
          String(fb.reason || "unavailable").replace(/_/g, " ") + resetTxt;
        banner.classList.remove("hidden");
      },
    };
  }

  
  function createCooldownOverlay(doc, composerEl, inputEl) {
    var overlay = el(doc, "div", "cooldown-overlay hidden");
    var txt = el(doc, "span", "cd-text");
    overlay.appendChild(txt);
    composerEl.style.position = "relative";
    composerEl.appendChild(overlay);
    var timer = null;

    function tick(qs) {
      var ms = new Date(qs.cooldown_until).getTime() - Date.now();
      if (!(ms > 0)) {
        overlay.classList.add("hidden");
        inputEl.disabled = false;
        if (timer) { clearInterval(timer); timer = null; }
        return;
      }
      txt.textContent = "⏳ Cooling down - retry in " + fmtCountdown(qs.cooldown_until);
    }

    return {
      el: overlay,
      update: function (qs) {
        if (qs.cooldown_until && new Date(qs.cooldown_until).getTime() > Date.now()) {
          overlay.classList.remove("hidden");
          inputEl.disabled = true;
          tick(qs);
          if (!timer) timer = setInterval(function () { tick(qs); }, 1000);
        } else {
          if (timer) { clearInterval(timer); timer = null; }
          overlay.classList.add("hidden");
          inputEl.disabled = false;
        }
      },
    };
  }

  
  function updateQueueChip(doc, qs) {
    var chip = doc.getElementById("status-queue");
    if (!chip) return;
    if (qs.queue_depth > 0) {
      chip.textContent = "⏳ Queued (" + qs.queue_depth + ")";
      chip.classList.remove("hidden");
      chip.title = qs.queue_depth + " task(s) queued - auto-retry when quota frees up";
    } else {
      chip.textContent = "";
      chip.classList.add("hidden");
    }
  }

  
  var meter = null, banner = null, overlay = null;

  function init(doc) {
    var quotaHost = doc.getElementById("status-quota");
    var bannerHost = doc.getElementById("chat-pane");
    var composer = doc.getElementById("composer");
    var input = doc.getElementById("input");
    if (!quotaHost || !bannerHost || !composer || !input) return false;
    quotaHost.textContent = "";
    meter = createQuotaMeter(doc, quotaHost);
    banner = createFallbackBanner(doc, bannerHost);

    var first = bannerHost.firstChild;
    if (banner.el.parentNode !== bannerHost || banner.el.nextSibling !== first) {
      bannerHost.insertBefore(banner.el, first);
    }
    overlay = createCooldownOverlay(doc, composer, input);
    return true;
  }

  function update(qs) {
    if (!meter) return;
    if (!qs || qs.v !== 1) { return; }
    meter.update(qs);
    banner.update(qs);
    overlay.update(qs);
    updateQueueChip(meter.el.ownerDocument, qs);
  }

  
  function onEnvelope(env) {
    if (env && env.type === "quota_state" && env.quota) update(env.quota);
  }

  
  function isoPlus(minutes) {
    return new Date(Date.now() + minutes * 60000).toISOString();
  }
  var MOCK_STATES = {
    healthy: {
      v: 1,
      minute: { used: 4, limit: 20, resets_at: isoPlus(3) },
      daily: { used: 12, limit: 50, resets_at: isoPlus(240) },
      queue_depth: 0,
      active_model: "nemotron-3-nano:free",
      fallback: null,
      cooldown_until: null,
      policy_owner: "tbd_pending_issue_10",
    },
    approaching: {
      v: 1,
      minute: { used: 15, limit: 20, resets_at: isoPlus(2) },
      daily: { used: 36, limit: 50, resets_at: isoPlus(180) },
      queue_depth: 0,
      active_model: "nemotron-3-nano:free",
      fallback: null,
      cooldown_until: null,
      policy_owner: "tbd_pending_issue_10",
    },
    at_limit: {
      v: 1,
      minute: { used: 20, limit: 20, resets_at: isoPlus(1) },
      daily: { used: 50, limit: 50, resets_at: isoPlus(120) },
      queue_depth: 3,
      active_model: "kimi-k2:free",
      fallback: {
        active: true,
        from: "nemotron-3-nano:free",
        to: "kimi-k2:free",
        reason: "daily_cap_exhausted",
        at: new Date().toISOString(),
      },
      cooldown_until: null,
      policy_owner: "tbd_pending_issue_10",
    },
    cooldown: {
      v: 1,
      minute: { used: 19, limit: 20, resets_at: isoPlus(1) },
      daily: { used: 40, limit: 50, resets_at: isoPlus(120) },
      queue_depth: 1,
      active_model: "nemotron-3-nano:free",
      fallback: null,
      cooldown_until: new Date(Date.now() + 45000).toISOString(),
      policy_owner: "tbd_pending_issue_10",
    },
  };

  return {
    STATES: STATES,
    computeState: computeState,
    fmtCountdown: fmtCountdown,
    createQuotaMeter: createQuotaMeter,
    createFallbackBanner: createFallbackBanner,
    createCooldownOverlay: createCooldownOverlay,
    init: init,
    update: update,
    onEnvelope: onEnvelope,
    MOCK_STATES: MOCK_STATES,
  };
});

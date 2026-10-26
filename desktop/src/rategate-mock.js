/* ZenCode Desktop - RateGate UI wireup mock (W-10, 0.4-3 part 3).
 * Simulates a quota-state backend with fake data - NO real API calls, no
 * network, no Tauri invoke. Feeds synthetic `quota_state` envelopes into
 * RateGateUI.onEnvelope on a timer so the D8 RateGate UI wiring can be
 * developed and demoed before the engine exposes the real endpoint
 * (policy interface TBD, pending atzenn/zencode#10).
 *
 * The mock behaves like a live feed: usage ticks upward as synthetic tool
 * calls "consume" quota, windows reset on schedule, and it occasionally
 * walks through fallback and cooldown episodes. All numbers are fake.
 *
 * Usage (console or demo driver):
 *   RateGateWireupMock.start(document);        // live ticking feed
 *   RateGateWireupMock.scenario("at_limit");   // jump to a scenario
 *   RateGateWireupMock.stop();                 // halt the feed
 */
(function (root, factory) {
  if (typeof module !== "undefined" && module.exports) module.exports = factory();
  else root.RateGateWireupMock = factory();
})(typeof self !== "undefined" ? self : this, function () {
  "use strict";

  var SCENARIOS = ["healthy", "approaching", "at_limit", "cooldown"];

  /* Deterministic PRNG (mulberry32) so demos/tests are reproducible. */
  function makeRng(seed) {
    var a = seed >>> 0;
    return function () {
      a |= 0; a = (a + 0x6D2B79F5) | 0;
      var t = Math.imul(a ^ (a >>> 15), 1 | a);
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
  }

  function isoPlus(minutes) {
    return new Date(Date.now() + minutes * 60000).toISOString();
  }

  /* Build a synthetic QuotaState envelope payload for a scenario. */
  function buildQuota(name, rng) {
    var minuteLimit = 20, dailyLimit = 50;
    var minuteUsed, dailyUsed, queueDepth = 0, fallback = null, cooldownUntil = null;
    switch (name) {
      case "approaching":
        minuteUsed = 15; dailyUsed = 36;
        break;
      case "at_limit":
        minuteUsed = 20; dailyUsed = 50; queueDepth = 2 + Math.floor(rng() * 3);
        fallback = {
          active: true,
          from: "nemotron-3-nano:free",
          to: "kimi-k2:free",
          reason: "daily_cap_exhausted",
          at: new Date().toISOString(),
        };
        break;
      case "cooldown":
        minuteUsed = 19; dailyUsed = 40; queueDepth = 1;
        cooldownUntil = new Date(Date.now() + 45000).toISOString();
        break;
      default: // healthy
        minuteUsed = 3 + Math.floor(rng() * 4);
        dailyUsed = 10 + Math.floor(rng() * 6);
    }
    return {
      v: 1,
      minute: { used: minuteUsed, limit: minuteLimit, resets_at: isoPlus(3) },
      daily: { used: dailyUsed, limit: dailyLimit, resets_at: isoPlus(240) },
      queue_depth: queueDepth,
      active_model: fallback ? fallback.to : "nemotron-3-nano:free",
      fallback: fallback,
      cooldown_until: cooldownUntil,
      policy_owner: "mock_wireup_no_backend",
    };
  }

  function envelope(seq, quota) {
    return { v: 1, seq: seq, type: "quota_state", quota: quota };
  }

  var timer = null;
  var seq = 0;
  var rng = makeRng(0xC0DE);
  var current = null; // live mutable QuotaState between ticks

  function push(env) {
    if (typeof RateGateUI !== "undefined" && RateGateUI.onEnvelope) {
      RateGateUI.onEnvelope(env);
    }
  }

  /* Advance the live feed one tick: fake tool calls consume quota. */
  function tick() {
    if (!current) current = buildQuota("healthy", rng);
    // 0-3 synthetic tool calls per tick, each costs 1 minute-quota unit.
    var calls = Math.floor(rng() * 4);
    current.minute.used = Math.min(current.minute.limit, current.minute.used + calls);
    if (calls > 0) {
      current.daily.used = Math.min(current.daily.limit, current.daily.used + 1);
    }
    current.queue_depth = Math.max(0, current.queue_depth + (rng() < 0.2 ? 1 : 0) - (rng() < 0.3 ? 1 : 0));
    // When the minute window fills, trigger a fake fallback episode.
    if (current.minute.used >= current.minute.limit && !current.fallback) {
      current.fallback = {
        active: true,
        from: "nemotron-3-nano:free",
        to: "kimi-k2:free",
        reason: "minute_cap_exhausted",
        at: new Date().toISOString(),
      };
      current.active_model = current.fallback.to;
    }
    push(envelope(seq++, current));
    return current;
  }

  function start(doc, opts) {
    opts = opts || {};
    stop();
    if (typeof RateGateUI === "undefined") return false;
    if (!RateGateUI.init(doc || (typeof document !== "undefined" ? document : null))) return false;
    rng = makeRng(opts.seed === undefined ? 0xC0DE : opts.seed);
    seq = 0;
    current = buildQuota(opts.scenario && SCENARIOS.indexOf(opts.scenario) >= 0 ? opts.scenario : "healthy", rng);
    push(envelope(seq++, current));
    timer = setInterval(tick, opts.intervalMs || 1000);
    return true;
  }

  function stop() {
    if (timer) { clearInterval(timer); timer = null; }
    current = null;
  }

  /* Jump the live feed (or a one-shot render) to a named scenario. */
  function scenario(name) {
    if (SCENARIOS.indexOf(name) < 0) return null;
    current = buildQuota(name, rng);
    push(envelope(seq++, current));
    return current;
  }

  function isRunning() { return timer !== null; }

  return {
    SCENARIOS: SCENARIOS,
    buildQuota: buildQuota,
    envelope: envelope,
    tick: tick,
    start: start,
    stop: stop,
    scenario: scenario,
    isRunning: isRunning,
  };
});

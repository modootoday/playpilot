// Renders the public /v1/state subset into #fleet-status. Text only, never HTML.
(function () {
  "use strict";
  var root = document.getElementById("fleet-status");
  if (!root) return;
  var src = root.getAttribute("data-src") || "status.json";
  var UNKNOWN = "unknown";

  function el(tag, text) {
    var e = document.createElement(tag);
    if (text !== undefined) e.textContent = String(text);
    return e;
  }

  function count(n) {
    return typeof n === "number" && n >= 0 && Math.floor(n) === n ? n : null;
  }

  function when(s) {
    if (typeof s !== "string") return UNKNOWN;
    var t = Date.parse(s);
    return isNaN(t) ? UNKNOWN : new Date(t).toISOString().replace(".000Z", "Z");
  }

  function table(head, rows) {
    var t = el("table");
    var tr = el("tr");
    head.forEach(function (h) { tr.appendChild(el("th", h)); });
    var thead = el("thead");
    thead.appendChild(tr);
    t.appendChild(thead);
    var body = el("tbody");
    rows.forEach(function (r) {
      var row = el("tr");
      r.forEach(function (c) { row.appendChild(el("td", c)); });
      body.appendChild(row);
    });
    t.appendChild(body);
    return t;
  }

  function unavailable(why) {
    root.replaceChildren(el("p", "Status unavailable" + (why ? " (" + why + ")." : ".")));
  }

  function render(s) {
    if (!s || typeof s !== "object" || !Array.isArray(s.nodes) || !Array.isArray(s.lanes)) {
      unavailable("the snapshot is not the public state");
      return;
    }
    var out = [];
    out.push(el("p", "Snapshot taken " + when(s.generatedAt) + "."));

    var online = s.nodes.filter(function (n) { return n && n.status === "online"; }).length;
    var open = 0;
    var openKnown = true;
    s.lanes.forEach(function (l) {
      var n = count(l && l.leased);
      if (n === null) openKnown = false; else open += n;
    });
    var summary = el("ul");
    summary.appendChild(el("li", "Nodes online: " + online + " of " + s.nodes.length));
    summary.appendChild(el("li", "Open claims: " + (openKnown ? open : UNKNOWN)));
    out.push(summary);

    out.push(el("h2", "Lanes"));
    if (s.lanes.length === 0) {
      out.push(el("p", "No lanes reported."));
    } else {
      out.push(table(["Lane", "State", "Queued", "Open claims"], s.lanes.map(function (l) {
        var state = l.paused === true ? "paused" : l.paused === false ? "active" : UNKNOWN;
        var q = count(l.queued);
        var c = count(l.leased);
        return [l.name || UNKNOWN, state, q === null ? UNKNOWN : q, c === null ? UNKNOWN : c];
      })));
    }

    out.push(el("h2", "Last run per project"));
    if (!Array.isArray(s.projects)) {
      out.push(el("p", "Unknown: the Worker does not publish project runs yet."));
    } else if (s.projects.length === 0) {
      out.push(el("p", "No projects reported."));
    } else {
      out.push(table(["Project", "Last run"], s.projects.map(function (p) {
        return [p.name || UNKNOWN, when(p.lastRun)];
      })));
    }
    root.replaceChildren.apply(root, out);
  }

  if (typeof fetch !== "function") {
    unavailable("this browser cannot fetch it");
    return;
  }
  fetch(src, { headers: { Accept: "application/json" }, credentials: "omit", cache: "no-store" })
    .then(function (r) {
      if (!r.ok) throw new Error("HTTP " + r.status);
      return r.json();
    })
    .then(render)
    .catch(function () { unavailable(""); });
})();

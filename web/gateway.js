(function (global) {
  "use strict";

  var MODAL_STYLE = {
    overlay: "position:fixed;inset:0;z-index:2147483000;background:rgba(0,0,0,.55);display:flex;align-items:center;justify-content:center;padding:16px",
    dialog: "position:relative;background:#fff;border-radius:12px;max-width:480px;width:100%;max-height:100%;overflow:auto",
    close: "position:absolute;top:6px;right:6px;z-index:1;min-width:44px;min-height:44px;border:0;border-radius:22px;background:#fff;color:#000;font:24px/1 sans-serif;cursor:pointer"
  };

  function open(options) {
    if (!options || !options.gateway || !options.paymentId) {
      throw new Error("gateway and paymentId are required");
    }
    var base = String(options.gateway).replace(/\/+$/, "");
    var gatewayOrigin = new URL(base).origin;
    var modal = options.mode === "modal";
    var container = null;
    if (!modal) {
      container = typeof options.container === "string" ? document.querySelector(options.container) : options.container;
      if (!container) throw new Error("container not found");
    }

    var frame = document.createElement("iframe");
    frame.src = base + "/embed/" + encodeURIComponent(options.paymentId);
    frame.title = options.title || "Payment";
    frame.setAttribute("allow", "clipboard-write");
    frame.style.cssText = "border:0;width:100%;min-height:420px;display:block";

    var overlay = null;
    var opener = null;
    var closed = false;

    if (modal) {
      opener = document.activeElement;
      overlay = document.createElement("div");
      overlay.style.cssText = MODAL_STYLE.overlay;
      var dialog = document.createElement("div");
      dialog.setAttribute("role", "dialog");
      dialog.setAttribute("aria-modal", "true");
      dialog.setAttribute("aria-label", frame.title);
      dialog.style.cssText = MODAL_STYLE.dialog;
      var x = document.createElement("button");
      x.type = "button";
      x.setAttribute("aria-label", options.closeLabel || "Close");
      x.textContent = "\u00d7";
      x.style.cssText = MODAL_STYLE.close;
      x.addEventListener("click", function () { api.close(); });
      overlay.addEventListener("mousedown", function (ev) { if (ev.target === overlay) api.close(); });
      dialog.appendChild(x);
      dialog.appendChild(frame);
      overlay.appendChild(dialog);
      document.body.appendChild(overlay);
      x.focus();
      global.document.addEventListener("keydown", onKey);
    } else {
      container.appendChild(frame);
    }

    function onKey(ev) {
      if (ev.key === "Escape") api.close();
    }

    function fail(code, message) {
      if (options.onError) options.onError({ code: code, message: message });
    }

    frame.addEventListener("error", function () { fail("frame_error", "the payment frame failed to load"); });

    function handler(ev) {
      if (ev.origin !== gatewayOrigin || ev.source !== frame.contentWindow) return;
      var m = ev.data;
      if (!m || m.source !== "payment-gateway" || m.payment_id !== options.paymentId) return;
      switch (m.type) {
        case "pgw:resize":
          if (typeof m.height === "number" && m.height > 0) frame.style.height = m.height + "px";
          break;
        case "pgw:ready":
          if (options.onReady) options.onReady();
          break;
        case "pgw:paid":
          if (options.onPaid) options.onPaid(m);
          break;
        case "pgw:expired":
          if (options.onExpired) options.onExpired(m);
          break;
        case "pgw:cancelled":
          if (options.onCancelled) options.onCancelled(m);
          break;
        case "pgw:error":
          fail(String(m.code || "error"), String(m.message || ""));
          break;
      }
    }
    global.addEventListener("message", handler);

    var api = {
      frame: frame,
      close: function () {
        if (closed) return;
        closed = true;
        global.removeEventListener("message", handler);
        global.document.removeEventListener("keydown", onKey);
        if (modal && overlay && overlay.parentNode) overlay.parentNode.removeChild(overlay);
        else if (frame.parentNode) frame.parentNode.removeChild(frame);
        if (modal && opener && opener.focus) opener.focus();
        if (options.onClosed) options.onClosed();
      }
    };
    return api;
  }

  global.PaymentGateway = { open: open };
})(window);

(function (global) {
  "use strict";

  function open(options) {
    if (!options || !options.gateway || !options.paymentId) {
      throw new Error("gateway and paymentId are required");
    }
    var base = String(options.gateway).replace(/\/+$/, "");
    var gatewayOrigin = new URL(base).origin;
    var container = typeof options.container === "string" ? document.querySelector(options.container) : options.container;
    if (!container) throw new Error("container not found");

    var frame = document.createElement("iframe");
    frame.src = base + "/embed/" + encodeURIComponent(options.paymentId);
    frame.title = "Payment";
    frame.setAttribute("allow", "clipboard-write");
    frame.style.cssText = "border:0;width:100%;min-height:420px;display:block";
    container.appendChild(frame);

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
      }
    }
    global.addEventListener("message", handler);

    return {
      frame: frame,
      close: function () {
        global.removeEventListener("message", handler);
        if (frame.parentNode) frame.parentNode.removeChild(frame);
      }
    };
  }

  global.PaymentGateway = { open: open };
})(window);

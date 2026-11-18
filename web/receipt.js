(function () {
  "use strict";
  var btn = document.getElementById("print");
  if (btn) btn.addEventListener("click", function () { window.print(); });
})();

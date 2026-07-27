(function () {
  'use strict';

  var scene = document.querySelector('.home-scene');
  if (!scene) return;

  // Hover and focus are the same intent — "this is the host I care about" —
  // and both funnel into one attribute the stylesheet reads. Delegated, so the
  // handler count does not grow with the rail.
  var hostOf = function (target) {
    var el = target && target.closest ? target.closest('.scene-host') : null;
    return el ? el.getAttribute('data-host') : null;
  };

  var activate = function (event) {
    var host = hostOf(event.target);
    if (host) {
      scene.setAttribute('data-active', host);
    } else {
      scene.removeAttribute('data-active');
    }
  };

  var clear = function () {
    scene.removeAttribute('data-active');
  };

  scene.addEventListener('pointerover', activate);
  scene.addEventListener('pointerleave', clear);
  // `focusin`/`focusout` rather than focus/blur: the focusable thing is a link
  // inside the card, not the card.
  scene.addEventListener('focusin', activate);
  scene.addEventListener('focusout', clear);

  // A reduced-motion request is a request not to have the animation at all.
  // The stylesheet already renders the stages in their lit end state, so there
  // is nothing left to start and no observer worth installing.
  var still = window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)');
  if (still && still.matches) return;

  // Below the fold by definition — running the pips while nobody is looking is
  // pure battery. One observer toggles the class both ways, so scrolling back
  // past the band stops it again.
  if (!('IntersectionObserver' in window)) {
    scene.classList.add('is-running');
    return;
  }

  var observer = new IntersectionObserver(function (entries) {
    for (var j = 0; j < entries.length; j++) {
      scene.classList.toggle('is-running', entries[j].isIntersecting);
    }
  }, { threshold: 0.15 });

  observer.observe(scene);
})();

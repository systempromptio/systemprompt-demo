import {
  MIN_TIME_MS,
  SCROLL_MILESTONES,
  buildEngagementData,
  calculateScrollVelocity,
  createState,
  detectRageClick,
  getScrollDepth,
  sendEvent
} from './analytics-helpers.js';

function initAnalytics() {
  const state = createState();

  function sendPageView() {
    if (state.pageViewSent) return;
    state.pageViewSent = true;

    sendEvent('page_view', {
      referrer: document.referrer || null,
      title: document.title
    });
  }

  function sendPageExit() {
    if (state.dataSent) return;

    const timeOnPage = Date.now() - state.pageLoadTime;
    if (timeOnPage < MIN_TIME_MS) return;

    state.dataSent = true;
    sendEvent('page_exit', buildEngagementData(state));
  }

  function sendScrollMilestone(milestone) {
    if (state.scrollMilestonesSent[milestone]) return;
    state.scrollMilestonesSent[milestone] = true;

    sendEvent('scroll', {
      depth: state.maxScrollDepth,
      milestone: milestone,
      direction: state.lastScrollDirection || 'down',
      velocity: calculateScrollVelocity(state)
    });
  }

  function sendLinkClick(targetUrl, linkText, isExternal) {
    sendEvent('link_click', {
      target_url: targetUrl,
      link_text: linkText ? linkText.substring(0, 100) : null,
      is_external: isExternal
    });
  }

  function recordFirstInteraction() {
    if (!state.firstInteractionTime) {
      state.firstInteractionTime = Date.now();
    }
  }

  function handleScroll() {
    const currentDepth = getScrollDepth();
    const currentPosition = window.scrollY;
    const currentTime = Date.now();

    if (!state.firstScrollTime) {
      state.firstScrollTime = currentTime;
      recordFirstInteraction();
    }

    if (currentDepth > state.maxScrollDepth) {
      state.maxScrollDepth = currentDepth;

      for (let i = 0; i < SCROLL_MILESTONES.length; i++) {
        const milestone = SCROLL_MILESTONES[i];
        if (currentDepth >= milestone && !state.scrollMilestonesSent[milestone]) {
          sendScrollMilestone(milestone);
        }
      }
    }

    if (state.scrollPositions.length > 0) {
      const lastPosition = state.scrollPositions[state.scrollPositions.length - 1].position;
      const direction = currentPosition > lastPosition ? 'down' : 'up';

      if (state.lastScrollDirection && direction !== state.lastScrollDirection) {
        state.scrollDirectionChanges++;
      }

      state.lastScrollDirection = direction;
    }

    state.scrollPositions.push({ position: currentPosition, time: currentTime });

    if (state.scrollPositions.length > 50) {
      state.scrollPositions = state.scrollPositions.slice(-50);
    }
  }

  function handleClick(event) {
    state.clickCount++;
    recordFirstInteraction();
    detectRageClick(state, Date.now());

    const target = event.target;
    const link = target.tagName === 'A' ? target : target.closest('a');

    if (link && link.href) {
      const isExternal = link.hostname !== window.location.hostname;
      const linkText = link.textContent || link.innerText;
      sendLinkClick(link.href, linkText, isExternal);
    }

    const isInteractive = target.tagName === 'A' ||
                         target.tagName === 'BUTTON' ||
                         target.tagName === 'INPUT' ||
                         target.closest('a') ||
                         target.closest('button');

    if (!isInteractive && state.clickCount > 1) {
      state.hasDeadClick = true;
    }
  }

  function handleMouseMove(event) {
    if (state.lastMousePosition) {
      const dx = event.clientX - state.lastMousePosition.x;
      const dy = event.clientY - state.lastMousePosition.y;
      state.mouseDistance += Math.sqrt(dx * dx + dy * dy);
    }

    state.lastMousePosition = { x: event.clientX, y: event.clientY };
  }

  function handleKeydown() {
    state.keyboardEvents++;
    recordFirstInteraction();
  }

  function handleCopy() {
    state.copyEvents++;
    recordFirstInteraction();
  }

  function handleVisibilityChange() {
    const now = Date.now();
    const elapsed = now - state.lastVisibilityChange;

    if (state.isVisible) {
      state.visibleTime += elapsed;
      state.focusTime += elapsed;
    } else {
      state.hiddenTime += elapsed;
    }

    state.isVisible = !document.hidden;
    state.lastVisibilityChange = now;

    if (document.hidden) {
      state.tabSwitches++;
      sendPageExit();
    }
  }

  function handleFocus() {
    if (!state.isVisible) {
      const now = Date.now();
      state.hiddenTime += now - state.lastVisibilityChange;
      state.lastVisibilityChange = now;
      state.isVisible = true;
    }
  }

  function handleBlur() {
    state.blurCount++;

    if (state.isVisible) {
      const now = Date.now();
      state.visibleTime += now - state.lastVisibilityChange;
      state.focusTime += now - state.lastVisibilityChange;
      state.lastVisibilityChange = now;
      state.isVisible = false;
    }
  }

  function handlePageHide() {
    sendPageExit();
  }

  let scrollTimeout;
  function throttledScroll() {
    if (!scrollTimeout) {
      scrollTimeout = setTimeout(() => {
        scrollTimeout = null;
        handleScroll();
      }, 100);
    }
  }

  let mouseMoveTimeout;
  function throttledMouseMove(event) {
    if (!mouseMoveTimeout) {
      mouseMoveTimeout = setTimeout(() => {
        mouseMoveTimeout = null;
      }, 50);
      handleMouseMove(event);
    }
  }

  sendPageView();

  window.addEventListener('scroll', throttledScroll, { passive: true });
  window.addEventListener('click', handleClick, { passive: true });
  window.addEventListener('mousemove', throttledMouseMove, { passive: true });
  window.addEventListener('keydown', handleKeydown, { passive: true });
  document.addEventListener('copy', handleCopy);
  document.addEventListener('visibilitychange', handleVisibilityChange);
  window.addEventListener('focus', handleFocus);
  window.addEventListener('blur', handleBlur);
  window.addEventListener('pagehide', handlePageHide);
  window.addEventListener('beforeunload', sendPageExit);

  handleScroll();
}

if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', initAnalytics);
} else {
  initAnalytics();
}

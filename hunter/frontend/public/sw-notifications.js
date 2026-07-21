/* Hunter Live — desktop notification service worker.
 *
 * Page Notifications cannot show action buttons; Chromium only honors `actions`
 * via ServiceWorkerRegistration.showNotification. This SW is notification-only
 * (no offline/cache) and posts click targets (Floor / Trade) back to the live SPA.
 */

self.addEventListener('install', (event) => {
  event.waitUntil(self.skipWaiting());
});

self.addEventListener('activate', (event) => {
  event.waitUntil(self.clients.claim());
});

self.addEventListener('notificationclick', (event) => {
  const data = event.notification.data || {};
  event.notification.close();

  let href = typeof data.href === 'string' ? data.href : '/floor';
  if (event.action === 'trade' && typeof data.tradeHref === 'string') {
    href = data.tradeHref;
  }

  const absolute = new URL(href, self.location.origin).href;

  event.waitUntil(
    self.clients.matchAll({ type: 'window', includeUncontrolled: true }).then((list) => {
      for (const client of list) {
        if (!client.url.startsWith(self.location.origin)) continue;
        client.postMessage({ type: 'hunter-notification-navigate', href });
        if ('focus' in client) return client.focus();
      }
      if (self.clients.openWindow) return self.clients.openWindow(absolute);
      return undefined;
    }),
  );
});

// DeviceList, the face of cell:cr.devices@2. Every device the viewer's chain lets through, online or not; a device
// that stops answering stays, marked offline with when it was last seen, and its card changes without a reload.
import { DeviceList } from '../gen/CrDevices.ui';
import type { DeviceKind, DeviceRow } from '../gen/CrDevices';
import { show } from '@ui/rt';
import { ago, clear, exhaustive, h, num } from '@ui/kit/dom';
import { empty, failed, loading, sourcesLine } from '@ui/kit/faces';

function kindText(k: DeviceKind): string {
  switch (k.tag) {
    case 'Server':
      return 'server';
    case 'Desktop':
      return 'desktop';
    case 'Laptop':
      return 'laptop';
    case 'Phone':
      return 'phone';
    case 'Tablet':
      return 'tablet';
    case 'Unknown':
      return 'node';
    default:
      return exhaustive(k);
  }
}

function card(d: DeviceRow, now: number, open: (id: string) => void): HTMLElement {
  return h(
    'div',
    { class: `glass device${d.online ? '' : ' off'}`, attrs: { 'data-device': d.name, 'data-online': String(d.online) }, onclick: () => open(d.id) },
    h('div', { class: 'label' }, h('i', { class: d.online ? 'g' : 'r' }), `${kindText(d.kind)} · ${d.online ? 'online' : 'offline'}`),
    h('div', { class: 'dname', text: d.name }),
    h(
      'dl',
      {},
      h('dt', { text: 'Last seen' }), h('dd', { class: 'mono', text: d.online ? 'now' : ago(d.last_seen_ms, now) }),
      h('dt', { text: 'Answer' }), h('dd', { class: 'mono', text: d.rtt_ms === null ? '-' : `${d.rtt_ms} ms` }),
      h('dt', { text: 'Cells' }), h('dd', { class: 'mono', text: d.cells === null ? '-' : num(d.cells) }),
      h('dt', { text: 'Checked' }), h('dd', { class: 'mono', text: ago(d.checked_ms, now) }),
      // Parts: absent on the wire for a viewer without cr.devices:net / cr.devices:passkeys
      ...(show(d.tailscale, (t) => [h('dt', { text: 'Tailnet' }), h('dd', { class: 'mono', text: `${t.ip}${t.online ? '' : ' (down)'}` })]) ?? []),
      ...(show(d.passkeys, (p) => (p.length === 0 ? [] : [h('dt', { text: 'Keys' }), h('dd', { text: p.map((x) => x.label).join(', ') })])) ?? []),
      d.inventory ? h('dt', { text: 'Inventory' }) : null,
      d.inventory ? h('dd', { text: d.inventory.summary }) : null,
    ),
    d.why !== '' ? h('div', { class: 'why mono', text: d.why }) : null,
    h('div', { class: 'sub mono', text: d.id.slice(0, 16) }),
  );
}

export default DeviceList.render((v) => {
  const body = h('div', {}, loading());
  v.draw((d, why) => {
    if (d.rows === undefined) return clear(body, why === null ? loading() : failed(why));
    const now = Date.now();
    const rows = [...d.rows].sort((a, b) => Number(a.online) - Number(b.online) || a.name.localeCompare(b.name));
    const up = rows.filter((x) => x.online).length;
    clear(
      body,
      h(
        'div',
        { class: 'stats' },
        h('div', { class: 'glass ticks stat' }, h('div', { class: 'label' }, h('i', { class: 'g' }), 'Online'), h('div', { class: 'num acc' }, num(up), h('small', { text: ` / ${rows.length}` })), h('div', { class: 'sub', text: 'doors that answered the last probe' })),
        h('div', { class: 'glass stat' }, h('div', { class: 'label' }, h('i', { class: 'r' }), 'Offline'), h('div', { class: 'num', text: num(rows.length - up) }), h('div', { class: 'sub', text: 'listed with when they were last seen' })),
      ),
      rows.length === 0 ? empty('No devices here.') : h('div', { class: 'grid' }, ...rows.map((x) => card(x, now, v.emit.opened))),
      why === null ? null : failed(why),
      sourcesLine(d.sources, why === null),
    );
  });
  return h('section', { class: 'panel', attrs: { 'data-view': 'DeviceList' } }, body);
});

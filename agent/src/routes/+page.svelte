<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';
	import { getCurrentWebview } from '@tauri-apps/api/webview';
	import { listen } from '@tauri-apps/api/event';
	import { check as checkUpdate, type Update } from '@tauri-apps/plugin-updater';
	import { relaunch } from '@tauri-apps/plugin-process';

	type Slot = {
		id: string;
		target: string | null;
		label: string | null;
		icon: string | null;
	};

	type PcInfo = { ip: string; port: number };
	type SpecialLocation = { shell: string; label: string; icon: string | null };

	const ORANGE = '#FF7A29';
	const CX = 422;
	const CY = 195;
	// 궤도1은 버튼 바깥쪽 끝(R1_DARK+btn1R=32)이 캔버스 반높이(195=base)를
	// 절대 넘지 않도록 역산: 195-32=163. 궤도2는 화면 가장자리에 가려져도
	// 되니, 궤도1이 줄어든 비율(163/176)만큼만 같이 줄인다: 304*163/176≈282.
	const R1_DARK = 163;
	const R2_DARK = 282;
	const DASH1_BASE = 163;
	const DASH2_BASE = 282;
	const DONUT_BASE = 193;
	const DONUT2_BASE = 312;
	const INNER_COUNT = 6;
	const OUTER_COUNT = 10;
	// 모바일과 개수·자리를 맞추기 위해 궤도2의 첫 슬롯(s6)을 설정 아이콘으로 예약한다(배치 불가).
	const SETTINGS_SLOT_ID = 's6';

	let slots = $state<Slot[]>(
		Array.from({ length: 16 }, (_, i) => ({ id: `s${i}`, target: null, label: null, icon: null }))
	);
	let pcInfo = $state<PcInfo | null>(null);
	let specials = $state<SpecialLocation[]>([]);
	let draggingSpecial = $state<SpecialLocation | null>(null);
	let ghostPos = $state<{ x: number; y: number } | null>(null);
	let dragOverId = $state<string | null>(null);
	let errorMsg = $state<string | null>(null);

	// 리렌더를 유발하는 state는 힌트뿐. 회전·볼륨·시계·프레스 피드백은 DOM 직접 갱신.
	let hint = $state<'in' | 'out' | 'gone'>('in');
	let settingsOpen = $state(false);
	let autostartEnabled = $state(false);

	type PairRequest = { request_id: string; device_name: string };
	let pairRequests = $state<PairRequest[]>([]);

	let pendingUpdate: Update | null = null;
	let updateVersion = $state<string | null>(null);
	let updateState = $state<'idle' | 'installing' | 'error'>('idle');

	async function installUpdate() {
		if (!pendingUpdate) return;
		updateState = 'installing';
		try {
			await pendingUpdate.downloadAndInstall();
			await relaunch();
		} catch {
			updateState = 'error';
		}
	}

	let ringEl: HTMLDivElement | undefined = $state();
	let dash1El: HTMLDivElement | undefined = $state();
	let dash2El: HTMLDivElement | undefined = $state();
	let donutEl: HTMLDivElement | undefined = $state();
	let donut2El: HTMLDivElement | undefined = $state();
	let toastEl: HTMLDivElement | undefined = $state();
	let clockEl: HTMLButtonElement | undefined = $state();
	let volLayerEl: HTMLDivElement | undefined = $state();
	let clockLayerEl: HTMLDivElement | undefined = $state();
	let volNumEl: HTMLDivElement | undefined = $state();
	let dayEl: HTMLDivElement | undefined = $state();
	let hourEl: HTMLSpanElement | undefined = $state();
	let minEl: HTMLSpanElement | undefined = $state();
	let dateEl: HTMLDivElement | undefined = $state();
	let innerEls = $state<HTMLButtonElement[]>([]);
	let outerEls = $state<HTMLButtonElement[]>([]);
	let innerRip = $state<HTMLDivElement[]>([]);
	let outerRip = $state<HTMLDivElement[]>([]);

	let rot1 = 0;
	let rot2 = 0;
	let r1 = R1_DARK;
	let r2 = R2_DARK;
	const press: Record<string, number> = {};
	let volume = 65;
	let volumeActive = false;
	let ring2Active = false;

	let drag: {
		pointerId: number;
		ring: 0 | 1 | 2;
		lastAngle: number;
		lastTime: number;
		velocity: number;
		startX: number;
		startY: number;
		moved: boolean;
		tapTarget: HTMLElement | null;
		startVolume: number;
		accum: number;
		total: number;
	} | null = null;

	let inertia1 = 0;
	let inertia2 = 0;
	let burstStart: number | null = null;
	let burstPrevEase = 0;
	let burstMag = 0;
	// 우클릭(지우기) 직후 곧바로 딸려오는 click을 무시하기 위한 짧은 억제창.
	let suppressClickUntil = 0;
	let lastTs: number | null = null;
	let autoRaf = 0;
	let metricsCache: { cx: number; cy: number; sx: number; sy: number } | null = null;

	let hintTimer: ReturnType<typeof setTimeout>;
	let hintGoneTimer: ReturnType<typeof setTimeout>;
	let toastHideTimer: ReturnType<typeof setTimeout>;
	let tickTimer: ReturnType<typeof setTimeout>;

	const DAYS = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];
	const MONTHS = [
		'Jan',
		'Feb',
		'Mar',
		'Apr',
		'May',
		'Jun',
		'Jul',
		'Aug',
		'Sep',
		'Oct',
		'Nov',
		'Dec'
	];

	function innerSlots() {
		return slots.slice(0, INNER_COUNT);
	}
	function outerSlots() {
		return slots.slice(INNER_COUNT, INNER_COUNT + OUTER_COUNT);
	}

	async function loadSlots() {
		slots = await invoke<Slot[]>('get_slots');
	}
	async function loadPcInfo() {
		pcInfo = await invoke<PcInfo>('get_pc_info');
	}
	async function loadSpecials() {
		specials = await invoke<SpecialLocation[]>('list_special_locations');
	}
	async function loadAutostart() {
		autostartEnabled = await invoke<boolean>('get_autostart_enabled').catch(() => false);
	}
	async function toggleAutostart(e: Event) {
		const enabled = (e.target as HTMLInputElement).checked;
		autostartEnabled = enabled;
		await invoke('set_autostart_enabled', { enabled }).catch(() => {
			autostartEnabled = !enabled;
		});
	}
	function openSoundSettings() {
		invoke('open_sound_settings').catch(() => {});
	}

	type PairedDevice = { device_id: string; device_name: string; paired_at: number };
	let pairedDevices = $state<PairedDevice[]>([]);
	async function loadPairedDevices() {
		pairedDevices = await invoke<PairedDevice[]>('list_paired_devices').catch(() => []);
	}
	async function unpairDevice(deviceId: string) {
		pairedDevices = pairedDevices.filter((d) => d.device_id !== deviceId);
		await invoke('unpair_device', { deviceId }).catch(() => {});
	}
	function formatPairedAt(epochSecs: number): string {
		return new Date(epochSecs * 1000).toLocaleDateString(undefined, {
			year: 'numeric',
			month: 'short',
			day: 'numeric'
		});
	}

	function slotIdAtPoint(x: number, y: number): string | null {
		const el = document.elementFromPoint(x, y);
		const cell = el?.closest<HTMLElement>('[data-slot-id]');
		const id = cell?.dataset.slotId ?? null;
		return id === SETTINGS_SLOT_ID ? null : id;
	}

	async function assignPath(slotId: string, path: string) {
		errorMsg = null;
		try {
			await invoke('assign_button', { slotId, path });
			await loadSlots();
		} catch (e) {
			errorMsg = String(e);
		}
	}

	async function handleOsDrop(paths: string[], x: number, y: number) {
		const scale = window.devicePixelRatio || 1;
		const id = slotIdAtPoint(x / scale, y / scale);
		if (!id) return;
		const target = paths[0];
		if (!target) return;
		await assignPath(id, target);
	}

	async function dropSpecial(slotId: string) {
		const loc = draggingSpecial;
		draggingSpecial = null;
		if (!loc) return;
		errorMsg = null;
		try {
			await invoke('assign_special', { slotId, shell: loc.shell, label: loc.label });
			await loadSlots();
		} catch (e) {
			errorMsg = String(e);
		}
	}

	function startDragSpecial(e: PointerEvent, loc: SpecialLocation) {
		e.preventDefault();
		draggingSpecial = loc;
		ghostPos = { x: e.clientX, y: e.clientY };

		const onMove = (ev: PointerEvent) => {
			ghostPos = { x: ev.clientX, y: ev.clientY };
			dragOverId = slotIdAtPoint(ev.clientX, ev.clientY);
		};
		const onUp = (ev: PointerEvent) => {
			window.removeEventListener('pointermove', onMove);
			window.removeEventListener('pointerup', onUp);
			const id = slotIdAtPoint(ev.clientX, ev.clientY);
			ghostPos = null;
			dragOverId = null;
			if (id) dropSpecial(id);
			else draggingSpecial = null;
		};
		window.addEventListener('pointermove', onMove);
		window.addEventListener('pointerup', onUp);
	}

	async function clearSlot(id: string) {
		await invoke('clear_button', { slotId: id });
		await loadSlots();
	}

	function paintClock() {
		const now = new Date();
		let h = now.getHours();
		const ampm = h >= 12 ? 'PM' : 'AM';
		h = h % 12;
		if (h === 0) h = 12;
		if (dayEl) dayEl.textContent = DAYS[now.getDay()];
		if (hourEl) hourEl.textContent = String(h).padStart(2, '0');
		if (minEl) minEl.textContent = String(now.getMinutes()).padStart(2, '0');
		if (dateEl) dateEl.textContent = `${MONTHS[now.getMonth()]} ${now.getDate()} · ${ampm}`;
	}

	function scheduleTick() {
		const delay = 1000 - (Date.now() % 1000);
		tickTimer = setTimeout(() => {
			paintClock();
			scheduleTick();
		}, delay);
	}

	function dismissHint() {
		if (hint !== 'in') return;
		clearTimeout(hintTimer);
		hint = 'out';
		hintGoneTimer = setTimeout(() => (hint = 'gone'), 500);
	}

	function showToast(text: string) {
		const el = toastEl;
		if (!el) return;
		el.textContent = text;
		el.style.opacity = '1';
		clearTimeout(toastHideTimer);
		toastHideTimer = setTimeout(() => {
			el.style.opacity = '0';
		}, 1300);
	}

	function place(
		els: HTMLButtonElement[],
		n: number,
		radius: number,
		rotation: number,
		half: number,
		pressKeyPrefix: string,
		now: number
	) {
		for (let i = 0; i < n; i++) {
			const el = els[i];
			if (!el) continue;
			const a = ((-90 + i * (360 / n) + rotation) * Math.PI) / 180;
			const x = CX + radius * Math.cos(a) - half;
			const y = CY + radius * Math.sin(a) - half;
			let scale = 1;
			const key = pressKeyPrefix + i;
			const p = press[key];
			if (p != null) {
				const t = (now - p) / 260;
				if (t >= 1) delete press[key];
				else scale = 1 - 0.11 * Math.sin(Math.PI * t);
			}
			el.style.transform = `translate3d(${x.toFixed(2)}px,${y.toFixed(2)}px,0) scale(${scale.toFixed(4)})`;
		}
	}

	function layout(now: number) {
		place(innerEls, INNER_COUNT, r1, rot1, 32, 'i', now);
		place(outerEls, OUTER_COUNT, r2, rot2, 30, 'o', now);
		if (dash1El) dash1El.style.transform = `scale(${(r1 / DASH1_BASE).toFixed(4)})`;
		if (dash2El) dash2El.style.transform = `scale(${(r2 / DASH2_BASE).toFixed(4)})`;
		if (donutEl) donutEl.style.transform = `scale(${((r1 + 30) / DONUT_BASE).toFixed(4)})`;
		if (donut2El) donut2El.style.transform = `scale(${((r2 + 30) / DONUT2_BASE).toFixed(4)})`;
	}

	function autoSpin(ts: number) {
		if (lastTs == null) lastTs = ts;
		const dt = Math.min(48, ts - lastTs);
		lastTs = ts;
		const speed = 0.003;

		let burstDelta = 0;
		if (burstStart != null) {
			const t = Math.min(1, (ts - burstStart) / 650);
			const ease = 1 - Math.pow(1 - t, 4);
			burstDelta = (ease - burstPrevEase) * burstMag;
			burstPrevEase = ease;
			if (t >= 1) {
				burstStart = null;
				burstPrevEase = 0;
			}
		}

		const friction = Math.pow(0.994, dt);
		let mv1 = 0;
		let mv2 = 0;
		if ((!drag || drag.ring !== 1) && inertia1) {
			mv1 = inertia1 * dt;
			inertia1 *= friction;
			if (Math.abs(inertia1) < 0.0005) inertia1 = 0;
		}
		if ((!drag || drag.ring !== 2) && inertia2) {
			mv2 = inertia2 * dt;
			inertia2 *= friction;
			if (Math.abs(inertia2) < 0.0005) inertia2 = 0;
		}

		if (!drag || drag.ring !== 1) rot1 += -dt * speed - burstDelta + mv1;
		if (!drag || drag.ring !== 2) rot2 += dt * speed + burstDelta * (INNER_COUNT / OUTER_COUNT) + mv2;

		const tR1 = R1_DARK;
		const tR2 = R2_DARK;
		const k = 1 - Math.pow(0.0015, dt / 1000);
		r1 += (tR1 - r1) * k;
		r2 += (tR2 - r2) * k;
		if (Math.abs(tR1 - r1) < 0.05) r1 = tR1;
		if (Math.abs(tR2 - r2) < 0.05) r2 = tR2;

		layout(ts);
		autoRaf = requestAnimationFrame(autoSpin);
	}

	function snapBurst() {
		burstStart = performance.now();
		burstPrevEase = 0;
		burstMag = 360 / INNER_COUNT;
		if (clockEl) {
			clockEl.style.animation = 'none';
			void clockEl.offsetWidth;
			clockEl.style.animation = 'sd-btn 0.16s ease-out';
		}
	}

	function pressKey(slotId: string, label: string | null, prefix: string, i: number, el: HTMLButtonElement, rip: HTMLDivElement) {
		press[prefix + i] = performance.now();
		el.style.boxShadow = `0 0 0 3px #fff, 0 0 20px rgba(255,122,41,0.6)`;
		clearTimeout((el as any)._sdShadowTimer);
		(el as any)._sdShadowTimer = setTimeout(() => {
			el.style.boxShadow = '0 4px 12px rgba(0,0,0,0.4)';
		}, 200);
		if (rip) {
			rip.style.animation = 'none';
			void rip.offsetWidth;
			rip.style.animation = 'sd-ripple-soft 0.5s ease-out';
		}
		showToast(`Launching: ${label ?? slotId}…`);
		invoke('press_slot', { slotId }).catch(() => {});
	}

	let lastAppliedVolumeActive = false;
	function applyVolumeVisual(force = false) {
		if (!force && volumeActive === lastAppliedVolumeActive) return;
		lastAppliedVolumeActive = volumeActive;
		const active = volumeActive;
		if (clockEl) {
			clockEl.style.borderWidth = active ? '9px' : '3px';
			const base = '0 0 40px rgba(255,122,41,0.25)';
			clockEl.style.boxShadow = active ? `0 0 0 3px rgba(255,122,41,0.3), ${base}` : base;
		}
		if (volLayerEl) {
			volLayerEl.style.opacity = active ? '1' : '0';
			volLayerEl.style.transform = active ? 'scale(1)' : 'scale(0.85)';
		}
		if (clockLayerEl) {
			clockLayerEl.style.opacity = active ? '0' : '1';
			clockLayerEl.style.transform = active ? 'scale(0.85)' : 'scale(1)';
		}
		if (donutEl) donutEl.style.opacity = active ? '1' : '0';
		if (volNumEl) volNumEl.textContent = String(Math.round(volume));
	}

	function metrics() {
		const rect = ringEl!.getBoundingClientRect();
		return { cx: rect.left + rect.width / 2, cy: rect.top + rect.height / 2, sx: rect.width / 844, sy: rect.height / 390 };
	}
	function angleAt(clientX: number, clientY: number) {
		const m = metricsCache || metrics();
		return (Math.atan2((clientY - m.cy) / m.sy, (clientX - m.cx) / m.sx) * 180) / Math.PI;
	}

	function onRingDown(e: PointerEvent) {
		if (!ringEl) return;
		metricsCache = metrics();
		const m = metricsCache;
		const dist = Math.hypot((e.clientX - m.cx) / m.sx, (e.clientY - m.cy) / m.sy);
		const inner = (90 + r1) / 2;
		const mid = (r1 + r2) / 2;
		const outer = r2 + 55;
		const ring: 0 | 1 | 2 | -1 = dist < inner ? 0 : dist < mid ? 1 : dist < outer ? 2 : -1;
		if (ring === -1) {
			drag = null;
			return;
		}
		(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
		const ang = angleAt(e.clientX, e.clientY);
		drag = {
			pointerId: e.pointerId,
			ring,
			lastAngle: ang,
			lastTime: performance.now(),
			velocity: 0,
			startX: e.clientX,
			startY: e.clientY,
			moved: false,
			tapTarget: (e.target as HTMLElement)?.closest('button'),
			startVolume: volume,
			accum: ring === 1 ? rot1 : rot2,
			total: 0
		};
		if (ring === 1) inertia1 = 0;
		else if (ring === 2) inertia2 = 0;
		dismissHint();
	}

	function onRingMove(e: PointerEvent) {
		const d = drag;
		if (!d || e.pointerId !== d.pointerId) return;
		if (!d.moved) {
			if (Math.hypot(e.clientX - d.startX, e.clientY - d.startY) <= 6) return;
			d.moved = true;
			// 문턱을 넘는 순간을 새 기준각으로 삼아, 넘기 전 이동량이 한번에 반영되어 튀는 것을 막는다.
			d.lastAngle = angleAt(e.clientX, e.clientY);
			d.lastTime = performance.now();
			// 볼륨 표시는 데드존 통과 여부가 아니라 "지금 궤도1을 드래그 중인가"에만
			// 묶는다 - 안 그러면 데드존 경계 근처에서 손이 살짝 떨릴 때마다 가운데
			// 화면이 시계↔볼륨으로 깜빡인다.
			if (d.ring === 1) {
				volumeActive = true;
				applyVolumeVisual();
			}
		}
		if (d.ring === 0) return;

		const ang = angleAt(e.clientX, e.clientY);
		let dAng = ang - d.lastAngle;
		if (dAng > 180) dAng -= 360;
		if (dAng < -180) dAng += 360;
		d.accum += dAng;
		const now = performance.now();
		const dt = now - d.lastTime;
		if (dt > 0) d.velocity = dAng / dt;
		d.lastAngle = ang;
		d.lastTime = now;

		if (d.ring === 1) {
			rot1 = d.accum;
			d.total += dAng;
			const dead = 1.5;
			const eff = Math.sign(d.total) * Math.max(0, Math.abs(d.total) - dead);
			let vol = d.startVolume - (eff / 40) * 10;
			if (vol > 100) {
				vol = 100;
				d.total = -(dead + ((100 - d.startVolume) / 10) * 40);
			} else if (vol < 0) {
				vol = 0;
				d.total = dead + (d.startVolume / 10) * 40;
			}
			volume = vol;
			if (volNumEl) volNumEl.textContent = String(Math.round(vol));
			invoke('set_volume', { level: vol / 100 }).catch(() => {});
		} else {
			rot2 = d.accum;
			ring2Active = true;
			if (donut2El) donut2El.style.opacity = '1';
		}
	}

	function onRingUp() {
		const d = drag;
		drag = null;
		metricsCache = null;
		if (!d) return;

		if (!d.moved && d.tapTarget) {
			if (d.ring === 0) {
				snapBurst();
			} else {
				d.tapTarget.click();
			}
		}

		const clamp = (v: number) => Math.max(-1.5, Math.min(1.5, v || 0));
		if (d.ring === 1) {
			inertia1 = clamp(d.velocity);
			volumeActive = false;
			applyVolumeVisual();
		} else if (d.ring === 2) {
			inertia2 = clamp(d.velocity);
			ring2Active = false;
			if (donut2El) donut2El.style.opacity = '0';
		}
	}

	function onRingCancel() {
		if (!drag) return;
		drag = null;
		metricsCache = null;
		volumeActive = false;
		ring2Active = false;
		if (donut2El) donut2El.style.opacity = '0';
		applyVolumeVisual();
	}

	let unlistenLaunch: () => void;
	let unlistenDrop: () => void;
	let unlistenRemoteLayout: () => void;
	let unlistenPairRequest: () => void;
	let unlistenPairResolved: () => void;

	function respondToPair(requestId: string, approve: boolean) {
		pairRequests = pairRequests.filter((r) => r.request_id !== requestId);
		invoke(approve ? 'approve_pair' : 'deny_pair', { requestId }).catch(() => {});
	}

	onMount(() => {
		loadSlots();
		loadPcInfo();
		loadSpecials();
		loadAutostart();

		checkUpdate()
			.then((update) => {
				if (update?.available) {
					pendingUpdate = update;
					updateVersion = update.version;
				}
			})
			.catch(() => {});
		invoke<number>('get_volume')
			.then((v) => {
				volume = v * 100;
			})
			.catch(() => {});

		hintTimer = setTimeout(() => dismissHint(), 6000);
		paintClock();
		scheduleTick();
		autoRaf = requestAnimationFrame(autoSpin);

		listen('launched', () => {}).then((fn) => (unlistenLaunch = fn));

		// 폰이 볼륨을 바꾸면 PC 자신은 그 broadcast를 못 받으므로(자기 자신의 WS 클라이언트가
		// 아님), 별도 Tauri 이벤트로 받아서 로컬 volume을 맞춘다 - 안 그러면 다음에 PC에서
		// 직접 드래그할 때 dragStartVolume이 옛날 값이라 확 튄다.
		listen<string>('remote-layout', (event) => {
			try {
				const json = JSON.parse(event.payload);
				if (typeof json.volume === 'number' && drag?.ring !== 1) {
					volume = json.volume * 100;
					if (volNumEl) volNumEl.textContent = String(Math.round(volume));
				}
			} catch {
				// ignore
			}
		}).then((fn) => (unlistenRemoteLayout = fn));

		listen<PairRequest>('pair-request', (event) => {
			pairRequests = [...pairRequests, event.payload];
		}).then((fn) => (unlistenPairRequest = fn));

		// 사용자가 응답하지 않아도(타임아웃/연결끊김) 백엔드가 이 요청을 정리하면서
		// 같은 이벤트를 보낸다 - 안 그러면 죽은 연결의 요청이 큐에 유령처럼 남아
		// 실제 살아있는 요청을 계속 뒤로 밀어낸다.
		listen<string>('pair-resolved', (event) => {
			pairRequests = pairRequests.filter((r) => r.request_id !== event.payload);
		}).then((fn) => (unlistenPairResolved = fn));

		const webview = getCurrentWebview();
		webview
			.onDragDropEvent((event) => {
				if (event.payload.type === 'over') {
					const scale = window.devicePixelRatio || 1;
					const { x, y } = event.payload.position;
					dragOverId = slotIdAtPoint(x / scale, y / scale);
				} else if (event.payload.type === 'drop') {
					const { x, y } = event.payload.position;
					dragOverId = null;
					handleOsDrop(event.payload.paths, x, y);
				} else {
					dragOverId = null;
				}
			})
			.then((fn) => (unlistenDrop = fn));
	});

	onDestroy(() => {
		clearTimeout(tickTimer);
		clearTimeout(toastHideTimer);
		clearTimeout(hintTimer);
		clearTimeout(hintGoneTimer);
		if (typeof cancelAnimationFrame !== 'undefined') cancelAnimationFrame(autoRaf);
		unlistenLaunch?.();
		unlistenDrop?.();
		unlistenRemoteLayout?.();
		unlistenPairRequest?.();
		unlistenPairResolved?.();
	});
</script>

<main>
{#if updateVersion}
	<div class="update-banner">
		{#if updateState === 'installing'}
			<span>Installing update…</span>
		{:else}
			<span>Update v{updateVersion} available</span>
			<button onclick={installUpdate}>Install & Restart</button>
			{#if updateState === 'error'}
				<span class="update-error">Failed — try again later</span>
			{/if}
		{/if}
	</div>
{/if}
{#if pairRequests.length > 0}
	<div class="pair-overlay">
		<div class="pair-card">
			<div class="pair-title">New device wants to connect</div>
			<div class="pair-device">{pairRequests[0].device_name}</div>
			<div class="pair-actions">
				<button class="pair-deny" onclick={() => respondToPair(pairRequests[0].request_id, false)}>Deny</button>
				<button class="pair-allow" onclick={() => respondToPair(pairRequests[0].request_id, true)}>Allow</button>
			</div>
		</div>
	</div>
{/if}
{#if settingsOpen}
	<div class="settings-panel">
		<button class="back-button" onclick={() => (settingsOpen = false)}>← Back</button>
		<h2>Settings</h2>

		{#if pcInfo}
			<div class="settings-row">
				<span>Connection</span>
				<span class="settings-value">ws://{pcInfo.ip}:{pcInfo.port}/ws</span>
			</div>
		{/if}

		<div class="settings-row">
			<span>Launch at Windows startup</span>
			<input type="checkbox" checked={autostartEnabled} onchange={toggleAutostart} />
		</div>

		<button class="settings-button" onclick={openSoundSettings}>Open Windows Sound Settings</button>

		<h2>Paired Devices</h2>
		{#if pairedDevices.length === 0}
			<p class="settings-empty">No devices paired yet</p>
		{:else}
			{#each pairedDevices as device (device.device_id)}
				<div class="settings-row">
					<span>{device.device_name} <span class="settings-hint">since {formatPairedAt(device.paired_at)}</span></span>
					<button class="unpair-button" onclick={() => unpairDevice(device.device_id)}>Unpair</button>
				</div>
			{/each}
		{/if}
	</div>
{:else}
	<div
		bind:this={ringEl}
		onpointerdown={onRingDown}
		onpointermove={onRingMove}
		onpointerup={onRingUp}
		onpointercancel={onRingCancel}
		onlostpointercapture={onRingCancel}
		role="application"
		aria-label="AutoDeck radial dial"
		class="ring"
		style:background="radial-gradient(circle at 50% 50%, #1a1b1e 0%, #0a0a0b 70%)"
	>
		<div class="rays"></div>

		<div bind:this={donutEl} class="donut"></div>
		<div bind:this={donut2El} class="donut donut2"></div>
		<div bind:this={dash1El} class="dash dash1" style:border-color="rgba(255,122,41,0.25)"></div>
		<div bind:this={dash2El} class="dash dash2" style:border-color="rgba(255,122,41,0.15)"></div>

		{#if hint !== 'gone'}
			<div class="hint" style:opacity={hint === 'in' ? 1 : 0}>
				<span>Drag inner ring · Volume</span>
			</div>
		{/if}

		<div bind:this={toastEl} class="toast"></div>

		<button bind:this={clockEl} class="clock" aria-label="Clock and volume">
			<div bind:this={volLayerEl} class="clock-layer vol-layer">
				<div class="vol-label">Volume</div>
				<div bind:this={volNumEl} class="vol-num">65</div>
			</div>
			<div bind:this={clockLayerEl} class="clock-layer">
				<div bind:this={dayEl} class="day"></div>
				<div class="time">
					<span bind:this={hourEl}></span><span class="colon">:</span><span bind:this={minEl}></span>
				</div>
				<div bind:this={dateEl} class="date"></div>
			</div>
		</button>

		{#each outerSlots() as slot, i (slot.id)}
			{#if slot.id === SETTINGS_SLOT_ID}
				<button
					bind:this={outerEls[i]}
					class="orbit orbit2 settings-slot"
					aria-label="Settings"
					style:background="#111214"
					onclick={() => {
						settingsOpen = true;
						loadPairedDevices();
					}}
				>
					⚙
				</button>
			{:else}
				<button
					bind:this={outerEls[i]}
					data-slot-id={slot.id}
					class="orbit orbit2"
					class:drag-over={dragOverId === slot.id}
					aria-label={slot.label ?? 'Empty slot'}
					style:background="#111214"
					onpointerdown={(e) => {
						if (e.button === 2) suppressClickUntil = performance.now() + 500;
					}}
					onclick={() => {
						if (performance.now() < suppressClickUntil) return;
						if (slot.target) pressKey(slot.id, slot.label, 'o', i, outerEls[i], outerRip[i]);
					}}
					oncontextmenu={(e) => {
						e.preventDefault();
						suppressClickUntil = performance.now() + 500;
						if (slot.target) clearSlot(slot.id);
					}}
				>
					{#if slot.icon}
						<img src={slot.icon} alt={slot.label ?? ''} />
					{/if}
					<div bind:this={outerRip[i]} class="ripple"></div>
				</button>
			{/if}
		{/each}

		{#each innerSlots() as slot, i (slot.id)}
			<button
				bind:this={innerEls[i]}
				data-slot-id={slot.id}
				class="orbit orbit1"
				class:drag-over={dragOverId === slot.id}
				aria-label={slot.label ?? 'Empty slot'}
				style:background="#111214"
				onpointerdown={(e) => {
					if (e.button === 2) suppressClickUntil = performance.now() + 500;
				}}
				onclick={() => {
					if (performance.now() < suppressClickUntil) return;
					if (slot.target) pressKey(slot.id, slot.label, 'i', i, innerEls[i], innerRip[i]);
				}}
				oncontextmenu={(e) => {
					e.preventDefault();
					suppressClickUntil = performance.now() + 500;
					if (slot.target) clearSlot(slot.id);
				}}
			>
				{#if slot.icon}
					<img src={slot.icon} alt={slot.label ?? ''} />
				{/if}
				<div bind:this={innerRip[i]} class="ripple"></div>
			</button>
		{/each}
	</div>

	{#if pcInfo}
		<p class="pc-info">ws://{pcInfo.ip}:{pcInfo.port}/ws</p>
	{/if}

	{#if errorMsg}
		<p class="error">{errorMsg}</p>
	{/if}

	{#if specials.length > 0}
		<div class="tray">
			{#each specials as loc (loc.shell)}
				<div
					class="tray-item"
					title={loc.label}
					role="img"
					aria-label={loc.label}
					onpointerdown={(e) => startDragSpecial(e, loc)}
				>
					{#if loc.icon}
						<img src={loc.icon} alt={loc.label} />
					{/if}
				</div>
			{/each}
		</div>
	{/if}

	{#if draggingSpecial && ghostPos}
		<div class="drag-ghost" style="left:{ghostPos.x}px; top:{ghostPos.y}px;">
			{#if draggingSpecial.icon}
				<img src={draggingSpecial.icon} alt="" />
			{/if}
		</div>
	{/if}
{/if}
</main>

<style>
	:global(html, body) {
		margin: 0;
		height: 100%;
		background: #14151a;
		color: #e8e8ec;
		font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Helvetica, Arial, sans-serif;
		user-select: none;
	}

	@keyframes -global-sd-blink {
		0%,
		49% {
			opacity: 1;
		}
		50%,
		100% {
			opacity: 0;
		}
	}
	@keyframes -global-sd-btn {
		0% {
			transform: scale(1);
		}
		40% {
			transform: scale(0.94);
		}
		100% {
			transform: scale(1);
		}
	}
	@keyframes -global-sd-ripple-soft {
		0% {
			transform: scale(1);
			opacity: 0.22;
		}
		100% {
			transform: scale(1.32);
			opacity: 0;
		}
	}

	main {
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: 12px;
		padding: 16px;
		box-sizing: border-box;
		height: 100%;
		position: relative;
	}

	.ring {
		width: 844px;
		height: 390px;
		max-width: 100%;
		overflow: hidden;
		border-radius: 22px;
		box-shadow: 0 14px 30px rgba(0, 0, 0, 0.25);
		position: relative;
		box-sizing: border-box;
		touch-action: none;
		cursor: grab;
		transition: background 0.3s ease;
	}

	.rays {
		position: absolute;
		inset: 0;
		background: repeating-conic-gradient(
			from 0deg,
			rgba(255, 122, 41, 0.04) 0deg 11.25deg,
			transparent 11.25deg 22.5deg
		);
		transition: opacity 0.3s ease;
		pointer-events: none;
	}

	.donut {
		position: absolute;
		left: 229px;
		top: 2px;
		width: 386px;
		height: 386px;
		border-radius: 50%;
		background: transparent;
		border: 60px solid rgba(255, 122, 41, 0.09);
		pointer-events: none;
		box-sizing: border-box;
		opacity: 0;
		transform-origin: 50% 50%;
		transition: opacity 0.18s ease;
	}

	.donut2 {
		left: 110px;
		top: -117px;
		width: 624px;
		height: 624px;
		border-color: rgba(255, 122, 41, 0.09);
	}

	.dash {
		position: absolute;
		border-radius: 50%;
		border: 1px dashed;
		pointer-events: none;
		transform-origin: 50% 50%;
		transition: border-color 0.3s ease;
	}
	.dash1 {
		left: 259px;
		top: 32px;
		width: 326px;
		height: 326px;
	}
	.dash2 {
		left: 140px;
		top: -87px;
		width: 564px;
		height: 564px;
	}

	.hint {
		position: absolute;
		left: 50%;
		top: 305px;
		transform: translateX(-50%);
		display: flex;
		align-items: center;
		gap: 10px;
		color: #6d7076;
		font-size: 10px;
		font-weight: 700;
		letter-spacing: 0.5px;
		text-transform: uppercase;
		pointer-events: none;
		white-space: nowrap;
		transition: opacity 0.5s ease;
		z-index: 4;
	}

	.toast {
		position: absolute;
		left: 50%;
		top: 296px;
		transform: translateX(-50%);
		background: rgba(0, 0, 0, 0.75);
		color: #fff;
		font-size: 11px;
		font-weight: 600;
		padding: 6px 14px;
		border-radius: 999px;
		pointer-events: none;
		white-space: nowrap;
		z-index: 5;
		opacity: 0;
		transition: opacity 0.18s ease;
	}

	.clock {
		outline: none;
		border: 3px solid #ff7a29;
		cursor: pointer;
		padding: 0;
		position: absolute;
		left: 332px;
		top: 105px;
		width: 180px;
		height: 180px;
		border-radius: 50%;
		background: #111214;
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: 2px;
		box-shadow: 0 0 40px rgba(255, 122, 41, 0.25);
		transition:
			border-width 0.12s ease,
			background 0.3s ease,
			box-shadow 0.18s ease;
	}

	.clock-layer {
		position: absolute;
		inset: 0;
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: 2px;
		opacity: 1;
		transform: scale(1);
		transition:
			opacity 0.22s ease,
			transform 0.22s ease;
		pointer-events: none;
	}
	.vol-layer {
		opacity: 0;
		transform: scale(0.85);
	}
	.vol-label {
		color: #8a8d92;
		font-size: 11px;
		font-weight: 700;
		letter-spacing: 0.6px;
		text-transform: uppercase;
	}
	.vol-num {
		font-family: 'SF Mono', ui-monospace, Menlo, monospace;
		font-size: 56px;
		font-weight: 700;
		color: #ff7a29;
		line-height: 1;
	}
	.day {
		color: #8a8d92;
		font-size: 13px;
		font-weight: 700;
		letter-spacing: 0.6px;
		text-transform: uppercase;
	}
	.time {
		font-family: 'SF Mono', ui-monospace, Menlo, monospace;
		font-size: 34px;
		font-weight: 700;
		color: #ff7a29;
		line-height: 1;
		display: flex;
	}
	.colon {
		animation: sd-blink 1s step-start infinite;
	}
	.date {
		color: #5a5d63;
		font-size: 13px;
		font-weight: 700;
	}

	.orbit {
		outline: none;
		border: 3px solid #ff7a29;
		cursor: pointer;
		padding: 0;
		border-radius: 50%;
		position: absolute;
		left: 0;
		top: 0;
		box-shadow: 0 4px 12px rgba(0, 0, 0, 0.4);
		display: flex;
		align-items: center;
		justify-content: center;
		transition:
			background 0.3s ease,
			box-shadow 0.2s ease;
	}
	.orbit1 {
		width: 64px;
		height: 64px;
	}
	.orbit2 {
		width: 60px;
		height: 60px;
	}
	.settings-slot {
		font-size: 30px;
		color: #ff7a29;
	}
	.orbit.drag-over {
		box-shadow: 0 0 0 3px #5b8cff;
	}
	.orbit img {
		width: 62%;
		height: 62%;
		pointer-events: none;
	}

	.ripple {
		position: absolute;
		inset: -2px;
		border-radius: 50%;
		border: 1.5px solid #ff7a29;
		pointer-events: none;
		opacity: 0;
	}

	.pc-info {
		margin: 0;
		font-size: 11px;
		color: #8a8a96;
		font-family: 'Cascadia Code', Consolas, monospace;
	}

	.update-banner {
		position: absolute;
		top: 10px;
		left: 50%;
		transform: translateX(-50%);
		z-index: 40;
		display: flex;
		align-items: center;
		gap: 10px;
		background: #111214;
		border: 1px solid #2a2b33;
		border-radius: 999px;
		padding: 6px 14px;
		font-size: 12px;
		color: #cfcfd6;
		box-shadow: 0 8px 20px rgba(0, 0, 0, 0.35);
	}
	.update-banner button {
		background: #ff7a29;
		color: #111214;
		border: none;
		border-radius: 999px;
		padding: 5px 12px;
		font-size: 12px;
		font-weight: 600;
		cursor: pointer;
	}
	.update-error {
		color: #ff6b6b;
	}

	.error {
		color: #ff6b6b;
		font-size: 13px;
		margin: 0;
	}

	.settings-panel {
		width: 844px;
		max-width: 100%;
		min-height: 390px;
		background: #111214;
		border-radius: 22px;
		box-shadow: 0 14px 30px rgba(0, 0, 0, 0.25);
		padding: 24px 32px;
		box-sizing: border-box;
		color: #e8e8ec;
	}

	.pair-overlay {
		position: absolute;
		inset: 0;
		background: rgba(8, 8, 10, 0.94);
		display: flex;
		align-items: center;
		justify-content: center;
		z-index: 50;
	}
	.pair-card {
		width: 320px;
		background: #111214;
		border: 1px solid #2a2b33;
		border-radius: 18px;
		box-shadow: 0 14px 30px rgba(0, 0, 0, 0.4);
		padding: 24px;
		text-align: center;
		color: #e8e8ec;
	}
	.pair-title {
		font-size: 13px;
		color: #8a8a96;
		margin-bottom: 8px;
	}
	.pair-device {
		font-size: 18px;
		font-weight: 600;
		margin-bottom: 20px;
	}
	.pair-actions {
		display: flex;
		gap: 10px;
	}
	.pair-actions button {
		flex: 1;
		padding: 10px 0;
		border-radius: 10px;
		font-size: 14px;
		cursor: pointer;
		border: 1px solid #2a2b33;
	}
	.pair-deny {
		background: #1c1d24;
		color: #cfcfd6;
	}
	.pair-allow {
		background: #ff7a29;
		color: #111214;
		font-weight: 600;
		border-color: #ff7a29;
	}
	.settings-panel h2 {
		margin: 8px 0 20px;
		font-size: 18px;
	}
	.back-button {
		background: #1c1d24;
		border: 1px solid #2a2b33;
		color: #cfcfd6;
		padding: 8px 14px;
		border-radius: 8px;
		cursor: pointer;
		font-size: 13px;
	}
	.back-button:hover {
		background: #24252d;
	}
	.settings-row {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 14px 0;
		border-bottom: 1px solid #24252d;
		font-size: 14px;
	}
	.settings-value {
		color: #8a8a96;
		font-family: 'Cascadia Code', Consolas, monospace;
		font-size: 12px;
	}
	.settings-button {
		margin-top: 20px;
		background: #1c1d24;
		border: 1px solid #2a2b33;
		color: #e8e8ec;
		padding: 10px 16px;
		border-radius: 8px;
		cursor: pointer;
		font-size: 14px;
	}
	.settings-button:hover {
		background: #24252d;
	}
	.settings-panel h2 {
		margin-top: 28px;
	}
	.settings-empty {
		color: #6d7076;
		font-size: 13px;
	}
	.settings-hint {
		color: #6d7076;
		font-size: 11px;
		font-weight: 400;
	}
	.unpair-button {
		background: #1c1d24;
		border: 1px solid #2a2b33;
		color: #ff8a8a;
		padding: 6px 12px;
		border-radius: 8px;
		cursor: pointer;
		font-size: 12px;
	}
	.unpair-button:hover {
		background: #2a1f1f;
	}

	.tray {
		display: flex;
		gap: 10px;
		padding: 8px 12px;
		border-radius: 12px;
		background: #1c1d24;
		border: 1px solid #2a2b33;
	}

	.tray-item {
		width: 32px;
		height: 32px;
		border-radius: 8px;
		display: flex;
		align-items: center;
		justify-content: center;
		cursor: grab;
		touch-action: none;
	}
	.tray-item:active {
		cursor: grabbing;
	}
	.tray-item img {
		width: 26px;
		height: 26px;
		pointer-events: none;
	}

	.drag-ghost {
		position: fixed;
		width: 40px;
		height: 40px;
		margin-left: -20px;
		margin-top: -20px;
		pointer-events: none;
		z-index: 50;
		opacity: 0.85;
	}
	.drag-ghost img {
		width: 100%;
		height: 100%;
	}
</style>

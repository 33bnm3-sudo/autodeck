package com.autodeck.app

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RadialGradient
import android.graphics.RectF
import android.graphics.Shader
import android.graphics.Typeface
import android.os.SystemClock
import android.util.AttributeSet
import android.util.Base64
import android.view.Choreographer
import android.view.MotionEvent
import android.view.View
import android.view.ViewConfiguration
import java.text.SimpleDateFormat
import java.util.Calendar
import java.util.Locale
import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.atan2
import kotlin.math.cos
import kotlin.math.hypot
import kotlin.math.max
import kotlin.math.min
import kotlin.math.pow
import kotlin.math.sign
import kotlin.math.sin

data class DeckButton(val id: String, val label: String?, val icon: String?) {
    var bitmap: Bitmap? = null
}

class RadialDeckView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null
) : View(context, attrs) {

    companion object {
        private const val ORANGE = "#FF7A29"
        private const val INNER_COUNT = 6
        private const val OUTER_COUNT = 10
        // 원본 844x390 캔버스(반높이 195) 기준 비율.
        // 궤도1은 버튼 바깥쪽 끝(r1+btn1R)이 화면의 짧은 변 절반(base)을 절대
        // 넘지 않도록 역산한 값 — 195(=base) - 32(=btn1R 비율) = 163.
        // 궤도2는 베젤에 가려져도 무방하니, 궤도1이 줄어든 비율(163/176)만큼만
        // 같이 줄여서 보기 좋게 맞춘다: 304 * 163/176 ≈ 282.
        private const val R1_DARK_RATIO = 163f / 195f
        private const val R2_DARK_RATIO = 282f / 195f
        private const val CLOCK_RATIO = 90f / 195f
        private const val BTN1_RATIO = 32f / 195f
        private const val BTN2_RATIO = 30f / 195f
        private const val OUTER_MARGIN_RATIO = 55f / 195f
        // 새 아이콘을 더 추가하는 게 아니라, 궤도2를 도는 아이콘 중 하나를 설정
        // 버튼으로 고정 배정한다 — 계속 같이 회전해야 자연스럽다(가만히 떠 있으면 어색함).
        // 프로토콜상 outerButtons[0]은 항상 id "s6"(slots.drop(6)의 첫 항목)이라
        // 이 id를 설정 슬롯으로 예약해서 쓴다.
        private const val SETTINGS_SLOT_ID = "s6"
    }

    var onPress: ((String) -> Unit)? = null
    var onVolumeChange: ((Float) -> Unit)? = null
    var onSettingsPress: (() -> Unit)? = null

    private var innerButtons: List<DeckButton> = List(INNER_COUNT) { DeckButton("s$it", null, null) }
    private var outerButtons: List<DeckButton> =
        List(OUTER_COUNT) { DeckButton("s${it + INNER_COUNT}", null, null) }

    private var cx = 0f
    private var cy = 0f
    private var base = 0f
    private var r1Dark = 0f
    private var r2Dark = 0f
    private var clockR = 0f
    private var btn1R = 0f
    private var btn2R = 0f
    private var outerMargin = 0f

    private var r1 = 0f
    private var r2 = 0f
    private var rot1 = 0.0
    private var rot2 = 0.0

    private var volume = 65f
    private var volumeActive = false
    private var volAlpha = 0f
    private var clockBorderW = 0f
    private var ring2Active = false
    private var ring2Alpha = 0f

    private val pressTimes = HashMap<String, Long>()
    private val appearTimes = HashMap<String, Long>()
    private var previousButtonsById: Map<String, DeckButton> = emptyMap()

    private var inertia1 = 0.0
    private var inertia2 = 0.0
    private var burstStart: Long = -1
    private var burstPrevEase = 0.0
    private var burstMag = 0.0
    private var lastFrameMs = 0L

    private var dragRing = -1 // -1 없음, 0 중앙, 1 안쪽, 2 바깥쪽
    private var dragLastAngle = 0.0
    private var dragLastTime = 0L
    private var dragVelocity = 0.0
    private var dragStartX = 0f
    private var dragStartY = 0f
    private var dragMoved = false
    private var dragStartVolume = 65f
    private var dragAccum = 0.0
    private var dragTotal = 0.0
    private var dragTapTarget: String? = null // 슬롯 id, 또는 중앙이면 "center"

    // 시스템 표준 터치 슬랍(밀도 대응). 이보다 작은 움직임은 손가락 떨림으로 보고
    // 완전히 무시한다 — 안 그러면 아이콘 탭이 살짝만 밀려도 회전/음량으로 오인된다.
    private val touchSlop = ViewConfiguration.get(context).scaledTouchSlop.toFloat()

    private val bgPaint = Paint(Paint.ANTI_ALIAS_FLAG)
    private val rayPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.FILL }
    private val dashPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeWidth = 2f
        pathEffect = android.graphics.DashPathEffect(floatArrayOf(6f, 6f), 0f)
    }
    private val donutPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.STROKE }
    private val circlePaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.FILL }
    private val borderPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        color = Color.parseColor(ORANGE)
    }
    private val iconPaint = Paint(Paint.ANTI_ALIAS_FLAG)
    private val settingsPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        textAlign = Paint.Align.CENTER
        color = Color.parseColor(ORANGE)
    }
    private val textPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        typeface = Typeface.MONOSPACE
        textAlign = Paint.Align.CENTER
        color = Color.parseColor(ORANGE)
    }
    private val labelPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        textAlign = Paint.Align.CENTER
        isFakeBoldText = true
    }

    private val dayFormat = SimpleDateFormat("EEEE", Locale.ENGLISH)
    private val dateFormat = SimpleDateFormat("MMM d", Locale.ENGLISH)

    private val frameCallback = object : Choreographer.FrameCallback {
        override fun doFrame(frameTimeNanos: Long) {
            step(frameTimeNanos / 1_000_000L)
            invalidate()
            if (isAttachedToWindow) Choreographer.getInstance().postFrameCallback(this)
        }
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        Choreographer.getInstance().postFrameCallback(frameCallback)
    }

    override fun onDetachedFromWindow() {
        Choreographer.getInstance().removeFrameCallback(frameCallback)
        super.onDetachedFromWindow()
    }

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        super.onSizeChanged(w, h, oldw, oldh)
        cx = w / 2f
        cy = h / 2f
        base = min(w, h) / 2f
        r1Dark = base * R1_DARK_RATIO
        r2Dark = base * R2_DARK_RATIO
        clockR = base * CLOCK_RATIO
        btn1R = base * BTN1_RATIO
        btn2R = base * BTN2_RATIO
        outerMargin = base * OUTER_MARGIN_RATIO
        if (r1 == 0f) {
            r1 = r1Dark
            r2 = r2Dark
        }
        clockBorderW = base * (3f / 195f)
        textPaint.textSize = base * (34f / 195f)
        labelPaint.textSize = base * (10f / 195f)
    }

    fun updateButtons(all: List<DeckButton>) {
        val inner = all.take(INNER_COUNT)
        val outer = all.drop(INNER_COUNT).take(OUTER_COUNT)
        inner.forEach { decodeIcon(it) }
        outer.forEach { decodeIcon(it) }
        // 첫 연결 때 배치된 아이콘들이 한꺼번에 뚝 나타나던 걸(페어링 직후 동기화
        // 등) 부드럽게 만들기 위해, 이전과 다른(새로 생겼거나 바뀐) 아이콘만
        // 골라 등장 애니메이션 타이머를 새로 건다 - 그대로인 아이콘은 매 레이아웃
        // 갱신마다 다시 깜빡이지 않는다.
        val now = System.currentTimeMillis()
        for (b in inner + outer) {
            val prev = previousButtonsById[b.id]
            if (b.icon != null && prev?.icon != b.icon) {
                appearTimes[b.id] = now
            }
        }
        previousButtonsById = (inner + outer).associateBy { it.id }
        innerButtons = inner
        outerButtons = outer
        invalidate()
    }

    fun updateVolumeBaseline(level: Float) {
        if (dragRing != 1) volume = level * 100f
    }

    private fun decodeIcon(b: DeckButton) {
        val data = b.icon ?: return
        try {
            val base64 = data.substringAfter(",")
            val bytes = Base64.decode(base64, Base64.DEFAULT)
            b.bitmap = BitmapFactory.decodeByteArray(bytes, 0, bytes.size)
        } catch (_: Exception) {
        }
    }

    // ── 물리 ──────────────────────────────────────────────────────────

    private fun step(nowMs: Long) {
        if (lastFrameMs == 0L) lastFrameMs = nowMs
        val dt = min(48L, nowMs - lastFrameMs).toDouble()
        lastFrameMs = nowMs
        val speed = 0.003

        var burstDelta = 0.0
        if (burstStart >= 0) {
            val t = min(1.0, (nowMs - burstStart) / 650.0)
            val ease = 1 - (1 - t).pow(4)
            burstDelta = (ease - burstPrevEase) * burstMag
            burstPrevEase = ease
            if (t >= 1.0) {
                burstStart = -1
                burstPrevEase = 0.0
            }
        }

        val friction = 0.994.pow(dt)
        var mv1 = 0.0
        var mv2 = 0.0
        if (dragRing != 1 && inertia1 != 0.0) {
            mv1 = inertia1 * dt
            inertia1 *= friction
            if (abs(inertia1) < 0.0005) inertia1 = 0.0
        }
        if (dragRing != 2 && inertia2 != 0.0) {
            mv2 = inertia2 * dt
            inertia2 *= friction
            if (abs(inertia2) < 0.0005) inertia2 = 0.0
        }

        if (dragRing != 1) rot1 += -dt * speed - burstDelta + mv1
        if (dragRing != 2) rot2 += dt * speed + burstDelta * (INNER_COUNT.toDouble() / OUTER_COUNT) + mv2

        val tR1 = r1Dark
        val tR2 = r2Dark
        val k = (1 - 0.0015.pow(dt / 1000.0)).toFloat()
        r1 += (tR1 - r1) * k
        r2 += (tR2 - r2) * k

        val targetVolAlpha = if (volumeActive) 1f else 0f
        volAlpha += (targetVolAlpha - volAlpha) * k
        val targetBorder = base * (if (volumeActive) 9f else 3f) / 195f
        clockBorderW += (targetBorder - clockBorderW) * k

        val targetRing2Alpha = if (ring2Active) 1f else 0f
        ring2Alpha += (targetRing2Alpha - ring2Alpha) * k
    }

    private fun angleFor(index: Int, n: Int, rotation: Double): Double {
        return Math.toRadians(-90.0 + index * (360.0 / n) + rotation)
    }

    private fun posFor(index: Int, n: Int, radius: Float, rotation: Double): Pair<Float, Float> {
        val a = angleFor(index, n, rotation)
        return Pair(cx + radius * cos(a).toFloat(), cy + radius * sin(a).toFloat())
    }

    private fun pressScale(key: String, now: Long): Float {
        val p = pressTimes[key] ?: return 1f
        val t = (now - p) / 260f
        if (t >= 1f) {
            pressTimes.remove(key)
            return 1f
        }
        return 1f - 0.11f * sin(PI.toFloat() * t)
    }

    // 아이콘이 막 동기화되어 나타날 때 0→1로 서서히 커지며 옅게 들어오는 진행률.
    private fun appearProgress(id: String, now: Long): Float {
        val t0 = appearTimes[id] ?: return 1f
        val t = (now - t0) / 260f
        if (t >= 1f) {
            appearTimes.remove(id)
            return 1f
        }
        return 1f - (1f - t).pow(3)
    }

    // ── 터치 ──────────────────────────────────────────────────────────

    override fun onTouchEvent(event: MotionEvent): Boolean {
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> onRingDown(event)
            MotionEvent.ACTION_MOVE -> onRingMove(event)
            MotionEvent.ACTION_UP -> onRingUp()
            MotionEvent.ACTION_CANCEL -> onRingCancel()
        }
        return true
    }

    private fun hitButton(px: Float, py: Float): String? {
        for (i in outerButtons.indices) {
            val (x, y) = posFor(i, OUTER_COUNT, r2, rot2)
            if (hypot((px - x).toDouble(), (py - y).toDouble()) <= btn2R) return outerButtons[i].id
        }
        for (i in innerButtons.indices) {
            val (x, y) = posFor(i, INNER_COUNT, r1, rot1)
            if (hypot((px - x).toDouble(), (py - y).toDouble()) <= btn1R) return innerButtons[i].id
        }
        return null
    }

    private fun angleAt(x: Float, y: Float): Double {
        return Math.toDegrees(atan2((y - cy).toDouble(), (x - cx).toDouble()))
    }

    private fun onRingDown(e: MotionEvent) {
        val dist = hypot((e.x - cx).toDouble(), (e.y - cy).toDouble())
        val innerT = (clockR + r1) / 2
        val midT = (r1 + r2) / 2
        val outerT = r2 + outerMargin
        val ring = when {
            dist < innerT -> 0
            dist < midT -> 1
            dist < outerT -> 2
            else -> -1
        }
        if (ring == -1) {
            dragRing = -1
            dragTapTarget = null
            return
        }
        parent?.requestDisallowInterceptTouchEvent(true)
        dragRing = ring
        dragLastAngle = angleAt(e.x, e.y)
        dragLastTime = System.currentTimeMillis()
        dragVelocity = 0.0
        dragStartX = e.x
        dragStartY = e.y
        dragMoved = false
        dragStartVolume = volume
        dragAccum = if (ring == 1) rot1 else rot2
        dragTotal = 0.0
        dragTapTarget = if (ring == 0) "center" else hitButton(e.x, e.y)
        if (ring == 1) inertia1 = 0.0 else if (ring == 2) inertia2 = 0.0
    }

    private fun onRingMove(e: MotionEvent) {
        if (dragRing == -1) return
        if (!dragMoved) {
            if (hypot((e.x - dragStartX).toDouble(), (e.y - dragStartY).toDouble()) <= touchSlop) {
                return // 슬랍 이내 흔들림은 회전/음량에 전혀 반영하지 않는다
            }
            dragMoved = true
            // 슬랍을 넘는 순간을 새 기준각으로 삼아, 문턱 넘기 전 이동량이
            // 한번에 훅 반영되어 튀는 것을 막는다.
            dragLastAngle = angleAt(e.x, e.y)
            dragLastTime = System.currentTimeMillis()
            // 볼륨 표시는 데드존 통과 여부가 아니라 "지금 궤도1을 드래그 중인가"에만
            // 묶는다 - 안 그러면 데드존 경계 근처에서 손이 살짝 떨릴 때마다 가운데
            // 화면이 시계↔볼륨으로 깜빡인다.
            if (dragRing == 1) volumeActive = true
        }
        if (dragRing == 0) return

        val ang = angleAt(e.x, e.y)
        var dAng = ang - dragLastAngle
        if (dAng > 180) dAng -= 360
        if (dAng < -180) dAng += 360
        dragAccum += dAng
        val now = System.currentTimeMillis()
        val dt = now - dragLastTime
        if (dt > 0) dragVelocity = dAng / dt
        dragLastAngle = ang
        dragLastTime = now

        if (dragRing == 1) {
            rot1 = dragAccum
            dragTotal += dAng
            val dead = 1.5
            val eff = sign(dragTotal) * max(0.0, abs(dragTotal) - dead)
            var vol = dragStartVolume - (eff / 40.0 * 10.0).toFloat()
            if (vol > 100f) {
                vol = 100f
                dragTotal = -(dead + (100.0 - dragStartVolume) / 10.0 * 40.0)
            } else if (vol < 0f) {
                vol = 0f
                dragTotal = dead + dragStartVolume / 10.0 * 40.0
            }
            volume = vol
            onVolumeChange?.invoke(vol / 100f)
        } else {
            rot2 = dragAccum
            ring2Active = true
        }
    }

    private fun onRingUp() {
        parent?.requestDisallowInterceptTouchEvent(false)
        val ring = dragRing
        val moved = dragMoved
        val target = dragTapTarget
        dragRing = -1

        if (!moved && target != null) {
            if (ring == 0) {
                snapBurst()
            } else if (target == SETTINGS_SLOT_ID) {
                onSettingsPress?.invoke()
            } else {
                val filled = (innerButtons + outerButtons).find { it.id == target }?.label != null
                if (filled) {
                    val prefix = if (ring == 1) "i" else "o"
                    val idx = if (ring == 1) innerButtons.indexOfFirst { it.id == target }
                    else outerButtons.indexOfFirst { it.id == target }
                    pressTimes[prefix + idx] = System.currentTimeMillis()
                    onPress?.invoke(target)
                }
            }
        }

        val clamp = { v: Double -> max(-1.5, min(1.5, v)) }
        if (ring == 1) {
            inertia1 = clamp(dragVelocity)
            volumeActive = false
        } else if (ring == 2) {
            inertia2 = clamp(dragVelocity)
            ring2Active = false
        }
    }

    private fun onRingCancel() {
        parent?.requestDisallowInterceptTouchEvent(false)
        dragRing = -1
        ring2Active = false
        volumeActive = false
    }

    private fun snapBurst() {
        burstStart = SystemClock.uptimeMillis()
        burstPrevEase = 0.0
        burstMag = 360.0 / INNER_COUNT
    }

    // ── 렌더 ──────────────────────────────────────────────────────────

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        drawBackground(canvas)
        drawRays(canvas)
        drawDonut(canvas)
        drawDashCircle(canvas, r1)
        drawDashCircle(canvas, r2)
        drawClock(canvas)
        drawRing(canvas, outerButtons, r2, rot2, btn2R, "o")
        drawRing(canvas, innerButtons, r1, rot1, btn1R, "i")
    }

    private fun drawBackground(canvas: Canvas) {
        val colors = intArrayOf(Color.parseColor("#1a1b1e"), Color.parseColor("#0a0a0b"))
        bgPaint.shader = RadialGradient(
            cx, cy, hypot(width.toDouble(), height.toDouble()).toFloat() * 0.6f,
            colors, floatArrayOf(0f, 1f), Shader.TileMode.CLAMP
        )
        canvas.drawRect(0f, 0f, width.toFloat(), height.toFloat(), bgPaint)
    }

    private fun drawRays(canvas: Canvas) {
        rayPaint.color = Color.argb(10, 255, 122, 41)
        val len = hypot(width.toDouble(), height.toDouble()).toFloat()
        for (i in 0 until 16) {
            val from = i * 22.5
            val to = from + 11.25
            val path = Path()
            path.moveTo(cx, cy)
            val steps = 6
            for (s in 0..steps) {
                val a = Math.toRadians(from + (to - from) * s / steps)
                path.lineTo(cx + len * cos(a).toFloat(), cy + len * sin(a).toFloat())
            }
            path.close()
            canvas.drawPath(path, rayPaint)
        }
    }

    private fun drawDonut(canvas: Canvas) {
        // PC의 .donut/.donut2는 box-sizing:border-box라 스케일 계산에 +30이 들어가도
        // 실제 렌더된 스트로크 중심선은 r1/r2와 정확히 일치한다(테두리 두께가 안쪽으로만
        // 깎이므로). 여기서도 오프셋 없이 그려야 버튼 궤도와 맞는다.
        if (volAlpha > 0.01f) {
            donutPaint.color = Color.argb((volAlpha * 255 * 0.6f).toInt(), 255, 122, 41)
            donutPaint.strokeWidth = base * (60f / 195f)
            canvas.drawCircle(cx, cy, r1, donutPaint)
        }
        if (ring2Alpha > 0.01f) {
            donutPaint.color = Color.argb((ring2Alpha * 255 * 0.6f).toInt(), 255, 122, 41)
            donutPaint.strokeWidth = base * (60f / 195f)
            canvas.drawCircle(cx, cy, r2, donutPaint)
        }
    }

    private fun drawDashCircle(canvas: Canvas, radius: Float) {
        // PC(+page.svelte)의 .dash1/.dash2는 오프셋 없이 r1/r2 그대로에 그려져
        // 버튼 궤도와 정확히 겹친다 — 여기서도 오프셋을 넣지 않아야 일치한다.
        dashPaint.color = Color.argb(64, 255, 122, 41)
        canvas.drawCircle(cx, cy, radius, dashPaint)
    }

    // PC(+page.svelte)의 .clock-layer flex column(gap:2px, day/date line-height~1.2, time line-height:1)을
    // 그대로 계산한 값. 세 줄을 175px 기준 세로로 중앙 정렬했을 때 각 줄 중심의 cy 기준 오프셋(base=195 비율).
    private fun drawCenteredText(canvas: Canvas, text: String, x: Float, centerY: Float, paint: Paint) {
        val fm = paint.fontMetrics
        val baseline = centerY - (fm.ascent + fm.descent) / 2f
        canvas.drawText(text, x, baseline, paint)
    }

    private fun drawClock(canvas: Canvas) {
        circlePaint.color = Color.parseColor("#111214")
        canvas.drawCircle(cx, cy, clockR, circlePaint)
        borderPaint.strokeWidth = clockBorderW
        canvas.drawCircle(cx, cy, clockR - clockBorderW / 2, borderPaint)

        val cal = Calendar.getInstance()
        var h = cal.get(Calendar.HOUR)
        if (h == 0) h = 12
        val m = cal.get(Calendar.MINUTE)
        val ampm = if (cal.get(Calendar.AM_PM) == 0) "AM" else "PM"

        if (volAlpha < 0.98f) {
            val a = ((1f - volAlpha) * 255).toInt()
            labelPaint.color = Color.argb(a, 138, 141, 146)
            labelPaint.textSize = base * (13f / 195f)
            drawCenteredText(canvas, dayFormat.format(cal.time), cx, cy - base * (27f / 195f), labelPaint)

            textPaint.alpha = a
            textPaint.textSize = base * (34f / 195f)
            val timeStr = "%02d:%02d".format(h, m)
            drawCenteredText(canvas, timeStr, cx, cy, textPaint)

            labelPaint.color = Color.argb(a, 90, 93, 99)
            labelPaint.textSize = base * (13f / 195f)
            drawCenteredText(canvas, "${dateFormat.format(cal.time)} · $ampm", cx, cy + base * (27f / 195f), labelPaint)
        }
        if (volAlpha > 0.02f) {
            val a = (volAlpha * 255).toInt()
            labelPaint.color = Color.argb(a, 138, 141, 146)
            labelPaint.textSize = base * (11f / 195f)
            drawCenteredText(canvas, "VOLUME", cx, cy - base * (29f / 195f), labelPaint)

            textPaint.alpha = a
            textPaint.textSize = base * (56f / 195f)
            drawCenteredText(canvas, volume.toInt().toString(), cx, cy + base * (7.5f / 195f), textPaint)
        }
    }

    private fun drawRing(
        canvas: Canvas,
        buttons: List<DeckButton>,
        radius: Float,
        rotation: Double,
        btnR: Float,
        prefix: String
    ) {
        val now = System.currentTimeMillis()
        for (i in buttons.indices) {
            val (x, y) = posFor(i, buttons.size, radius, rotation)
            val scale = pressScale(prefix + i, now)
            val rr = btnR * scale

            circlePaint.color = Color.parseColor("#111214")
            canvas.drawCircle(x, y, rr, circlePaint)
            borderPaint.strokeWidth = base * (3f / 195f)
            canvas.drawCircle(x, y, rr - borderPaint.strokeWidth / 2, borderPaint)

            if (buttons[i].id == SETTINGS_SLOT_ID) {
                settingsPaint.color = Color.parseColor(ORANGE)
                settingsPaint.textSize = rr * 1.15f
                val fm = settingsPaint.fontMetrics
                val baseline = y - (fm.ascent + fm.descent) / 2f
                canvas.drawText("⚙", x, baseline, settingsPaint)
            } else {
                val bmp = buttons[i].bitmap
                if (bmp != null) {
                    val appear = appearProgress(buttons[i].id, now)
                    val iconR = rr * 0.62f * (0.75f + 0.25f * appear)
                    val dst = RectF(x - iconR, y - iconR, x + iconR, y + iconR)
                    iconPaint.alpha = (appear * 255).toInt()
                    canvas.drawBitmap(bmp, null, dst, iconPaint)
                }
            }
        }
    }
}

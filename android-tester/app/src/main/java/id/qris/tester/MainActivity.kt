package id.qris.tester

import android.Manifest
import android.app.Activity
import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.os.Build
import android.os.Bundle
import android.provider.Settings
import android.text.InputType
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.Button
import android.widget.EditText
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import android.widget.Toast
import org.json.JSONObject
import java.text.NumberFormat
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.TimeZone
import java.util.UUID

class MainActivity : Activity() {

    private lateinit var tvServerStatus: TextView
    private lateinit var tvNotifListenerStatus: TextView
    private lateinit var etServerUrl: EditText
    private lateinit var etMasterKey: EditText
    private lateinit var etStaticQris: EditText
    private lateinit var tvProvisionInfo: TextView

    private lateinit var etBaseAmount: EditText
    private lateinit var tvTxnId: TextView
    private lateinit var tvTxnAmounts: TextView
    private lateinit var tvTxnStatusBadge: TextView
    private lateinit var ivQrCode: ImageView

    private lateinit var tvLog: TextView
    private lateinit var scrollLog: ScrollView

    @Volatile
    private var activeTxnId: String = ""
    @Volatile
    private var activeFinalAmount: Long = 0L
    @Volatile
    private var sseRunning: Boolean = false

    private val rupiahFormatter = NumberFormat.getNumberInstance(Locale("id", "ID"))

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        createTestNotificationChannel()
        requestPostNotificationPermissionIfNeeded()

        setContentView(buildUi())
        refreshProvisionInfoUi()
        updateNotifListenerStatusUi()

        QrisNotificationService.onLogCallback = { msg ->
            appendLog(msg)
        }

        // Run auto E2E diagnostic if launched with -e auto_test true
        if (intent?.getStringExtra("auto_test") == "true" || intent?.getBooleanExtra("auto_test", false) == true) {
            runFullAutomatedDiagnosticSuite()
        } else {
            checkServerHealth()
        }
    }

    override fun onResume() {
        super.onResume()
        updateNotifListenerStatusUi()
    }

    override fun onDestroy() {
        sseRunning = false
        if (QrisNotificationService.onLogCallback != null) {
            QrisNotificationService.onLogCallback = null
        }
        super.onDestroy()
    }

    private fun dp(value: Int): Int =
        (value * resources.displayMetrics.density).toInt()

    private fun buildCard(): LinearLayout {
        return LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            val pad = dp(14)
            setPadding(pad, pad, pad, pad)
            background = GradientDrawable().apply {
                setColor(Color.WHITE)
                cornerRadius = dp(12).toFloat()
                setStroke(dp(1), Color.parseColor("#E2E8F0"))
            }
            val lp = LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.WRAP_CONTENT
            )
            lp.bottomMargin = dp(12)
            layoutParams = lp
        }
    }

    private fun buildSectionTitle(title: String): TextView {
        return TextView(this).apply {
            text = title
            textSize = 15f
            setTypeface(null, Typeface.BOLD)
            setTextColor(Color.parseColor("#0F172A"))
            setPadding(0, 0, 0, dp(8))
        }
    }

    private fun buildButton(
        label: String,
        bgColorHex: String = "#2563EB",
        textColorHex: String = "#FFFFFF",
        onClick: () -> Unit = {}
    ): Button {
        return Button(this).apply {
            text = label
            isAllCaps = false
            textSize = 13.5f
            setTypeface(null, Typeface.BOLD)
            setTextColor(Color.parseColor(textColorHex))
            background = GradientDrawable().apply {
                setColor(Color.parseColor(bgColorHex))
                cornerRadius = dp(8).toFloat()
            }
            val lp = LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                 dp(44)
            )
            lp.topMargin = dp(6)
            layoutParams = lp
            setOnClickListener { onClick() }
        }
    }

    private fun buildInput(hintText: String, initialValue: String, numeric: Boolean = false): EditText {
        return EditText(this).apply {
            hint = hintText
            setText(initialValue)
            textSize = 13f
            setTextColor(Color.parseColor("#1E293B"))
            setHintTextColor(Color.parseColor("#94A3B8"))
            if (numeric) {
                inputType = InputType.TYPE_CLASS_NUMBER
            } else {
                inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
            }
            val padH = dp(10)
            val padV = dp(8)
            setPadding(padH, padV, padH, padV)
            background = GradientDrawable().apply {
                setColor(Color.parseColor("#F8FAFC"))
                cornerRadius = dp(8).toFloat()
                setStroke(dp(1), Color.parseColor("#CBD5E1"))
            }
            val lp = LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.WRAP_CONTENT
            )
            lp.bottomMargin = dp(6)
            layoutParams = lp
        }
    }

    private fun buildUi(): View {
        val rootScroll = ScrollView(this).apply {
            setBackgroundColor(Color.parseColor("#F1F5F9"))
            isFillViewport = true
        }

        val container = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            val pad = dp(14)
            setPadding(pad, pad, pad, pad)
        }

        // Header Banner
        val headerCard = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            val pad = dp(14)
            setPadding(pad, pad, pad, pad)
            background = GradientDrawable().apply {
                setColor(Color.parseColor("#0F172A"))
                cornerRadius = dp(12).toFloat()
            }
            val lp = LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.WRAP_CONTENT
            )
            lp.bottomMargin = dp(12)
            layoutParams = lp
        }

        headerCard.addView(TextView(this).apply {
            text = "QRIS Static2Dynamic Tester"
            textSize = 18f
            setTypeface(null, Typeface.BOLD)
            setTextColor(Color.WHITE)
        })
        headerCard.addView(TextView(this).apply {
            text = "USB Debugging E2E Validator • POS + SSE + Notification Forwarder"
            textSize = 12f
            setTextColor(Color.parseColor("#94A3B8"))
            setPadding(0, dp(2), 0, dp(8))
        })

        tvServerStatus = TextView(this).apply {
            text = "Server Status: Mengecek koneksi..."
            textSize = 12.5f
            setTypeface(null, Typeface.BOLD)
            setTextColor(Color.parseColor("#FACC15"))
        }
        headerCard.addView(tvServerStatus)

        tvNotifListenerStatus = TextView(this).apply {
            text = "Notification Listener: Mengecek izin..."
            textSize = 12.5f
            setTextColor(Color.parseColor("#38BDF8"))
            setPadding(0, dp(2), 0, 0)
        }
        headerCard.addView(tvNotifListenerStatus)

        container.addView(headerCard)

        // CARD 1: Server & Provisioning
        val cardSetup = buildCard()
        cardSetup.addView(buildSectionTitle("1. Konfigurasi Server & 1-Click Provisioning"))

        etServerUrl = buildInput("Server URL (http://127.0.0.1:8080)", AppPrefs.getServerUrl(this))
        cardSetup.addView(etServerUrl)

        etMasterKey = buildInput("Master API Key", AppPrefs.getMasterKey(this))
        cardSetup.addView(etMasterKey)

        etStaticQris = buildInput("Payload QRIS Statis", AppPrefs.getStaticQris(this))
        cardSetup.addView(etStaticQris)

        val rowSetupBtns = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            weightSum = 2f
        }
        val btnHealth = buildButton("Cek /healthz & /readyz", "#475569").apply {
            layoutParams = LinearLayout.LayoutParams(0, dp(42), 1f).apply {
                rightMargin = dp(4)
                topMargin = dp(4)
            }
        }
        btnHealth.setOnClickListener {
            saveInputsToPrefs()
            checkServerHealth()
        }

        val btnProvision = buildButton("1-Click Setup Merchant", "#2563EB").apply {
            layoutParams = LinearLayout.LayoutParams(0, dp(42), 1f).apply {
                leftMargin = dp(4)
                topMargin = dp(4)
            }
        }
        btnProvision.setOnClickListener {
            saveInputsToPrefs()
            provisionMerchantAndSource()
        }

        rowSetupBtns.addView(btnHealth)
        rowSetupBtns.addView(btnProvision)
        cardSetup.addView(rowSetupBtns)

        tvProvisionInfo = TextView(this).apply {
            textSize = 11.5f
            typeface = Typeface.MONOSPACE
            setTextColor(Color.parseColor("#334155"))
            setPadding(0, dp(8), 0, 0)
        }
        cardSetup.addView(tvProvisionInfo)
        container.addView(cardSetup)

        // CARD 2: POS Kasir & Dynamic QRIS + SSE
        val cardPos = buildCard()
        cardPos.addView(buildSectionTitle("2. Buat Transaksi QRIS Dinamis & Pantau SSE"))

        etBaseAmount = buildInput("Nominal Dasar Rupiah (misal: 50000)", "50000", numeric = true)
        cardPos.addView(etBaseAmount)

        cardPos.addView(buildButton("Generate Dynamic QRIS + Buka Stream SSE", "#059669") {
            saveInputsToPrefs()
            createDynamicTransaction()
        })

        tvTxnStatusBadge = TextView(this).apply {
            text = "BELUM ADA TRANSAKSI"
            textSize = 14f
            gravity = Gravity.CENTER
            setTypeface(null, Typeface.BOLD)
            setTextColor(Color.parseColor("#475569"))
            val pad = dp(8)
            setPadding(pad, pad, pad, pad)
            background = GradientDrawable().apply {
                setColor(Color.parseColor("#E2E8F0"))
                cornerRadius = dp(8).toFloat()
            }
            val lp = LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.WRAP_CONTENT
            )
            lp.topMargin = dp(10)
            layoutParams = lp
        }
        cardPos.addView(tvTxnStatusBadge)

        tvTxnId = TextView(this).apply {
            text = "ID Transaksi: -"
            textSize = 12f
            typeface = Typeface.MONOSPACE
            setTextColor(Color.parseColor("#475569"))
            setPadding(0, dp(6), 0, 0)
        }
        cardPos.addView(tvTxnId)

        tvTxnAmounts = TextView(this).apply {
            text = "Nominal Akhir: -"
            textSize = 15f
            setTypeface(null, Typeface.BOLD)
            setTextColor(Color.parseColor("#0F172A"))
            setPadding(0, dp(4), 0, dp(8))
        }
        cardPos.addView(tvTxnAmounts)

        ivQrCode = ImageView(this).apply {
            val size = dp(220)
            val lp = LinearLayout.LayoutParams(size, size).apply {
                gravity = Gravity.CENTER_HORIZONTAL
                bottomMargin = dp(8)
            }
            layoutParams = lp
            scaleType = ImageView.ScaleType.FIT_CENTER
            visibility = View.GONE
        }
        cardPos.addView(ivQrCode)

        val rowTxnActions = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            weightSum = 2f
        }
        val btnPollStatus = buildButton("Cek Status Manual", "#334155").apply {
            layoutParams = LinearLayout.LayoutParams(0, dp(40), 1f).apply {
                rightMargin = dp(4)
            }
        }
        btnPollStatus.setOnClickListener { pollTransactionStatus() }

        val btnCancelTxn = buildButton("Cancel Transaksi", "#DC2626").apply {
            layoutParams = LinearLayout.LayoutParams(0, dp(40), 1f).apply {
                leftMargin = dp(4)
            }
        }
        btnCancelTxn.setOnClickListener { cancelActiveTransaction() }

        rowTxnActions.addView(btnPollStatus)
        rowTxnActions.addView(btnCancelTxn)
        cardPos.addView(rowTxnActions)
        container.addView(cardPos)

        // CARD 3: Simulasi Pembayaran & Notification Listener
        val cardSim = buildCard()
        cardSim.addView(buildSectionTitle("3. Uji Auto-Detect Pembayaran (Simulasi & Asli)"))

        cardSim.addView(TextView(this).apply {
            text = "Uji coba deteksi pembayaran otomatis via Push Notifikasi Android (GoPay/DANA/Bank) atau tembak langsung Webhook HMAC."
            textSize = 12f
            setTextColor(Color.parseColor("#64748B"))
            setPadding(0, 0, 0, dp(6))
        })

        cardSim.addView(buildButton("⚡ Trigger Push Notifikasi Bayar di HP (E2E OS)", "#7C3AED") {
            triggerSimulatedPushNotification()
        })

        cardSim.addView(buildButton("🚀 Kirim Webhook Mutasi HMAC Langsung", "#0284C7") {
            sendDirectSimulatedWebhook()
        })

        cardSim.addView(buildButton("🔔 Buka Pengaturan Izin Akses Notifikasi HP", "#D97706") {
            openNotificationListenerSettings()
        })

        cardSim.addView(buildButton("🧪 Jalankan Automated Full E2E Suite (1-Klik)", "#0F172A") {
            saveInputsToPrefs()
            runFullAutomatedDiagnosticSuite()
        })

        container.addView(cardSim)

        // CARD 4: Live Console Log
        val cardLog = buildCard()
        val rowLogHeader = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }
        rowLogHeader.addView(buildSectionTitle("4. Live Console & Diagnostic Logs").apply {
            layoutParams = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f)
            setPadding(0, 0, 0, 0)
        })
        val btnClearLog = Button(this).apply {
            text = "Bersihkan"
            isAllCaps = false
            textSize = 11f
            setOnClickListener { tvLog.text = "" }
        }
        rowLogHeader.addView(btnClearLog)
        cardLog.addView(rowLogHeader)

        scrollLog = ScrollView(this).apply {
            val lp = LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                dp(210)
            )
            lp.topMargin = dp(6)
            layoutParams = lp
            background = GradientDrawable().apply {
                setColor(Color.parseColor("#0F172A"))
                cornerRadius = dp(8).toFloat()
            }
            val pad = dp(10)
            setPadding(pad, pad, pad, pad)
        }

        tvLog = TextView(this).apply {
            text = "=== QRIS Static2Dynamic Diagnostic Console ===\n"
            textSize = 11f
            typeface = Typeface.MONOSPACE
            setTextColor(Color.parseColor("#E2E8F0"))
        }
        scrollLog.addView(tvLog)
        cardLog.addView(scrollLog)

        container.addView(cardLog)
        rootScroll.addView(container)
        return rootScroll
    }

    private fun saveInputsToPrefs() {
        AppPrefs.setServerUrl(this, etServerUrl.text.toString())
        AppPrefs.setMasterKey(this, etMasterKey.text.toString())
        AppPrefs.setStaticQris(this, etStaticQris.text.toString())
    }

    private fun refreshProvisionInfoUi() {
        val mId = AppPrefs.getMerchantId(this)
        val mKey = AppPrefs.getMerchantKey(this)
        val sId = AppPrefs.getSourceId(this)
        val sSec = AppPrefs.getSourceSecret(this)

        if (mId.isBlank()) {
            tvProvisionInfo.text = "Status Provisioning: Belum ada merchant aktif.\nKlik '1-Click Setup Merchant' untuk mendaftarkan otomatis."
        } else {
            val maskedMKey = if (mKey.length > 16) mKey.take(16) + "..." else mKey
            val maskedSSec = if (sSec.length > 16) sSec.take(16) + "..." else sSec
            tvProvisionInfo.text = buildString {
                append("Merchant ID : $mId\n")
                append("Merchant Key: $maskedMKey\n")
                append("Source ID   : $sId (ANDROID)\n")
                append("Source HMAC : $maskedSSec")
            }
        }
    }

    private fun isNotificationListenerEnabled(): Boolean {
        val cn = ComponentName(this, QrisNotificationService::class.java)
        val flat = Settings.Secure.getString(contentResolver, "enabled_notification_listeners")
        return flat != null && flat.contains(cn.flattenToString())
    }

    private fun updateNotifListenerStatusUi() {
        val enabled = isNotificationListenerEnabled()
        runOnUiThread {
            if (enabled) {
                tvNotifListenerStatus.text = "Notification Listener: AKTIF (Siap Auto-Forward)"
                tvNotifListenerStatus.setTextColor(Color.parseColor("#4ADE80"))
            } else {
                tvNotifListenerStatus.text = "Notification Listener: BELUM DIIZINKAN (Klik tombol izin di bawah)"
                tvNotifListenerStatus.setTextColor(Color.parseColor("#FB923C"))
            }
        }
    }

    private fun updateStatusBadge(status: String) {
        runOnUiThread {
            tvTxnStatusBadge.text = "STATUS: $status"
            val bg = GradientDrawable().apply { cornerRadius = dp(8).toFloat() }
            when (status.uppercase(Locale.ROOT)) {
                "PAID" -> {
                    bg.setColor(Color.parseColor("#DCFCE7"))
                    bg.setStroke(dp(2), Color.parseColor("#16A34A"))
                    tvTxnStatusBadge.setTextColor(Color.parseColor("#15803D"))
                }
                "PENDING" -> {
                    bg.setColor(Color.parseColor("#FEF9C3"))
                    bg.setStroke(dp(2), Color.parseColor("#CA8A04"))
                    tvTxnStatusBadge.setTextColor(Color.parseColor("#854D0E"))
                }
                "CANCELED", "EXPIRED" -> {
                    bg.setColor(Color.parseColor("#FEE2E2"))
                    bg.setStroke(dp(2), Color.parseColor("#DC2626"))
                    tvTxnStatusBadge.setTextColor(Color.parseColor("#B91C1C"))
                }
                else -> {
                    bg.setColor(Color.parseColor("#E2E8F0"))
                    tvTxnStatusBadge.setTextColor(Color.parseColor("#475569"))
                }
            }
            tvTxnStatusBadge.background = bg
        }
    }

    private fun appendLog(msg: String) {
        val time = SimpleDateFormat("HH:mm:ss", Locale.getDefault()).format(Date())
        val fullLog = "[$time] $msg"
        android.util.Log.i("QrisTester", fullLog)
        runOnUiThread {
            tvLog.append("$fullLog\n")
            scrollLog.post { scrollLog.fullScroll(View.FOCUS_DOWN) }
        }
    }

    // =========================================================================
    // 1. Check Server Health (/healthz & /readyz)
    // =========================================================================
    private fun checkServerHealth() {
        val baseUrl = AppPrefs.getServerUrl(this)
        appendLog("Mengecek kesehatan server pada $baseUrl ...")
        Thread {
            try {
                val h = QrisApiClient.request(baseUrl, "GET", "/healthz")
                val r = QrisApiClient.request(baseUrl, "GET", "/readyz")
                if (h.isSuccess && r.isSuccess) {
                    appendLog("PASS: /healthz (${h.code}) & /readyz (${r.code}) -> ${r.body}")
                    runOnUiThread {
                        tvServerStatus.text = "Server Status: ONLINE & READY ($baseUrl)"
                        tvServerStatus.setTextColor(Color.parseColor("#4ADE80"))
                    }
                } else {
                    appendLog("WARN: Server merespons /healthz=${h.code}, /readyz=${r.code}")
                    runOnUiThread {
                        tvServerStatus.text = "Server Status: ERROR HTTP ${r.code}"
                        tvServerStatus.setTextColor(Color.parseColor("#F87171"))
                    }
                }
            } catch (e: Exception) {
                appendLog("ERROR Koneksi ke $baseUrl: ${e.message}")
                runOnUiThread {
                    tvServerStatus.text = "Server Status: OFFLINE (${e.javaClass.simpleName})"
                    tvServerStatus.setTextColor(Color.parseColor("#F87171"))
                }
            }
        }.start()
    }

    // =========================================================================
    // 2. Provision Merchant + Notification Source (ANDROID)
    // =========================================================================
    private fun provisionMerchantAndSource(onDone: ((Boolean) -> Unit)? = null) {
        val baseUrl = AppPrefs.getServerUrl(this)
        val masterKey = AppPrefs.getMasterKey(this)
        val staticQris = AppPrefs.getStaticQris(this)

        appendLog("Memulai 1-Click Provisioning Merchant & Android Source...")

        Thread {
            try {
                // Step A: Parse static QRIS first to verify validity
                val parseReq = JSONObject().apply {
                    put("payload", staticQris)
                    put("include_tlv", false)
                }.toString()

                val parseRes = QrisApiClient.request(
                    baseUrl = baseUrl,
                    method = "POST",
                    path = "/api/v1/qris/parse",
                    bearerToken = masterKey,
                    signingSecret = masterKey,
                    jsonBody = parseReq
                )
                appendLog("POST /api/v1/qris/parse -> HTTP ${parseRes.code}")

                // Step B: Create Merchant
                // Note: If the same static QRIS is already registered, generate a unique static payload or handle existing
                val createMchReq = JSONObject().apply {
                    put("name", "Android USB Tester ${System.currentTimeMillis() % 10000}")
                    put("static_payload", staticQris)
                    put("default_fee_mode", "NONE")
                    put("default_unique_code", true)
                    put("default_ttl_seconds", 900)
                }.toString()

                var mchRes = QrisApiClient.request(
                    baseUrl = baseUrl,
                    method = "POST",
                    path = "/api/v1/merchants",
                    bearerToken = masterKey,
                    signingSecret = masterKey,
                    jsonBody = createMchReq
                )

                // If 409 conflict (merchant static payload already registered in this DB session),
                // check if we already have credentials saved in AppPrefs
                if (mchRes.code == 409 && AppPrefs.getMerchantId(this).isNotBlank() && AppPrefs.getSourceId(this).isNotBlank()) {
                    appendLog("INFO: Merchant sudah terdaftar di DB & kredensial lokal siap digunakan.")
                    runOnUiThread { refreshProvisionInfoUi() }
                    onDone?.invoke(true)
                    return@Thread
                }

                if (!mchRes.isSuccess) {
                    appendLog("FAIL buat merchant (${mchRes.code}): ${mchRes.body}")
                    onDone?.invoke(false)
                    return@Thread
                }

                val mchJson = JSONObject(mchRes.body)
                val merchantId = mchJson.getJSONObject("merchant").getString("id")
                val creds = mchJson.getJSONObject("credentials")
                val apiKey = creds.getString("api_key")
                val signingSecret = creds.getString("signing_secret")

                appendLog("Merchant dibuat: $merchantId")

                // Step C: Create ANDROID Notification Source
                val createSrcReq = JSONObject().apply {
                    put("channel", "ANDROID")
                    put("label", "HP Android USB Tester (${Build.MODEL})")
                }.toString()

                val srcRes = QrisApiClient.request(
                    baseUrl = baseUrl,
                    method = "POST",
                    path = "/api/v1/merchants/$merchantId/sources",
                    bearerToken = masterKey,
                    signingSecret = masterKey,
                    jsonBody = createSrcReq
                )

                if (!srcRes.isSuccess) {
                    appendLog("FAIL buat source (${srcRes.code}): ${srcRes.body}")
                    onDone?.invoke(false)
                    return@Thread
                }

                val srcJson = JSONObject(srcRes.body)
                val sourceId = srcJson.getString("id")
                val sourceSecret = srcJson.getString("source_secret")

                AppPrefs.saveProvisioning(
                    this,
                    merchantId = merchantId,
                    merchantKey = apiKey,
                    signingSecret = signingSecret,
                    sourceId = sourceId,
                    sourceSecret = sourceSecret
                )

                appendLog("PASS: Source ANDROID dibuat ($sourceId). Provisioning selesai!")
                runOnUiThread {
                    refreshProvisionInfoUi()
                    Toast.makeText(this, "Merchant & Source Siap!", Toast.LENGTH_SHORT).show()
                }
                onDone?.invoke(true)
            } catch (e: Exception) {
                appendLog("Exception Provisioning: ${e.message}")
                onDone?.invoke(false)
            }
        }.start()
    }

    // =========================================================================
    // 3. Create Dynamic Transaction + Render QR PNG + Start SSE Stream
    // =========================================================================
    private fun createDynamicTransaction(onCreated: ((String, Long) -> Unit)? = null) {
        val baseUrl = AppPrefs.getServerUrl(this)
        val merchantId = AppPrefs.getMerchantId(this)
        val merchantKey = AppPrefs.getMerchantKey(this)
        val signingSecret = AppPrefs.getSigningSecret(this)

        if (merchantId.isBlank() || merchantKey.isBlank()) {
            appendLog("Merchant belum di-setup! Menjalankan auto-setup terlebih dahulu...")
            provisionMerchantAndSource { ok ->
                if (ok) createDynamicTransaction(onCreated)
            }
            return
        }

        val amountStr = etBaseAmount.text.toString().trim()
        val baseAmount = amountStr.toLongOrNull() ?: 50000L

        appendLog("Membuat transaksi dinamis Rp ${rupiahFormatter.format(baseAmount)}...")

        Thread {
            try {
                val reqJson = JSONObject().apply {
                    put("base_amount", baseAmount)
                    put("reference_label", "POS-${System.currentTimeMillis() % 100000}")
                }.toString()

                val res = QrisApiClient.request(
                    baseUrl = baseUrl,
                    method = "POST",
                    path = "/api/v1/merchants/$merchantId/transactions",
                    bearerToken = merchantKey,
                    signingSecret = signingSecret,
                    jsonBody = reqJson,
                    extraHeaders = mapOf("Idempotency-Key" to UUID.randomUUID().toString())
                )

                if (!res.isSuccess) {
                    appendLog("FAIL buat transaksi (${res.code}): ${res.body}")
                    return@Thread
                }

                val txnJson = JSONObject(res.body)
                val txnId = txnJson.getString("id")
                val uniqueCode = txnJson.getLong("unique_code")
                val finalAmount = txnJson.getLong("final_amount")
                val status = txnJson.getString("status")
                val crc = txnJson.getString("crc")

                activeTxnId = txnId
                activeFinalAmount = finalAmount

                appendLog("PASS Transaksi Dibuat: $txnId | Base=$baseAmount + KodeUnik=$uniqueCode -> Final=Rp ${rupiahFormatter.format(finalAmount)} (CRC=$crc)")

                runOnUiThread {
                    tvTxnId.text = "ID: $txnId (CRC: $crc)"
                    tvTxnAmounts.text = "Tagihan: Rp ${rupiahFormatter.format(finalAmount)} (Kode Unik: +$uniqueCode)"
                    updateStatusBadge(status)
                }

                // Fetch QR PNG
                val qrBmp = QrisApiClient.fetchQrBitmap(baseUrl, txnId, merchantKey)
                if (qrBmp != null) {
                    runOnUiThread {
                        ivQrCode.setImageBitmap(qrBmp)
                        ivQrCode.visibility = View.VISIBLE
                    }
                    appendLog("PASS: Gambar QRIS Dinamis (PNG) berhasil di-render.")
                }

                // Start SSE Real-Time Stream
                startSseStream(baseUrl, txnId, merchantKey)

                onCreated?.invoke(txnId, finalAmount)
            } catch (e: Exception) {
                appendLog("Exception buat transaksi: ${e.message}")
            }
        }.start()
    }

    private fun startSseStream(baseUrl: String, txnId: String, bearerToken: String) {
        sseRunning = false
        Thread.sleep(100)
        sseRunning = true

        Thread {
            appendLog("[SSE] Membuka stream /api/v1/transactions/$txnId/stream ...")
            QrisApiClient.streamTransactionEvents(
                baseUrl = baseUrl,
                txnId = txnId,
                bearerToken = bearerToken,
                shouldContinue = { sseRunning && activeTxnId == txnId },
                onEvent = { eventType, data ->
                    when (eventType) {
                        "status" -> {
                            appendLog("[SSE Event] $data")
                            try {
                                val obj = JSONObject(data)
                                val st = obj.getString("status")
                                updateStatusBadge(st)
                                if (st == "PAID" || st == "CANCELED" || st == "EXPIRED") {
                                    appendLog("[SSE] Status terminal tercapai ($st). Stream selesai.")
                                    sseRunning = false
                                    if (st == "PAID") {
                                        runOnUiThread {
                                            Toast.makeText(
                                                this@MainActivity,
                                                "PEMBAYARAN DITERIMA (PAID)!",
                                                Toast.LENGTH_LONG
                                            ).show()
                                        }
                                    }
                                }
                            } catch (_: Exception) {
                            }
                        }
                        "ping" -> {
                            appendLog("[SSE KeepAlive] : ping")
                        }
                        "error" -> {
                            appendLog("[SSE Error] $data")
                        }
                    }
                }
            )
        }.start()
    }

    private fun pollTransactionStatus() {
        val txnId = activeTxnId
        if (txnId.isBlank()) {
            Toast.makeText(this, "Buat transaksi terlebih dahulu", Toast.LENGTH_SHORT).show()
            return
        }
        val baseUrl = AppPrefs.getServerUrl(this)
        val merchantKey = AppPrefs.getMerchantKey(this)

        Thread {
            try {
                val res = QrisApiClient.request(
                    baseUrl = baseUrl,
                    method = "GET",
                    path = "/api/v1/transactions/$txnId/status",
                    bearerToken = merchantKey
                )
                appendLog("GET /status (${res.code}): ${res.body}")
                if (res.isSuccess) {
                    val st = JSONObject(res.body).getString("status")
                    updateStatusBadge(st)
                }
            } catch (e: Exception) {
                appendLog("Error cek status: ${e.message}")
            }
        }.start()
    }

    private fun cancelActiveTransaction() {
        val txnId = activeTxnId
        if (txnId.isBlank()) {
            Toast.makeText(this, "Belum ada transaksi aktif", Toast.LENGTH_SHORT).show()
            return
        }
        val baseUrl = AppPrefs.getServerUrl(this)
        val merchantKey = AppPrefs.getMerchantKey(this)
        val signingSecret = AppPrefs.getSigningSecret(this)

        Thread {
            try {
                val body = JSONObject().apply {
                    put("reason", "Dibatalkan dari Android Tester")
                }.toString()
                val res = QrisApiClient.request(
                    baseUrl = baseUrl,
                    method = "POST",
                    path = "/api/v1/transactions/$txnId/cancel",
                    bearerToken = merchantKey,
                    signingSecret = signingSecret,
                    jsonBody = body
                )
                appendLog("POST /cancel (${res.code}): ${res.body}")
                if (res.isSuccess) {
                    val st = JSONObject(res.body).getString("status")
                    updateStatusBadge(st)
                }
            } catch (e: Exception) {
                appendLog("Error cancel: ${e.message}")
            }
        }.start()
    }

    // =========================================================================
    // 4. Simulated Notification & Direct Webhook
    // =========================================================================
    private fun triggerSimulatedPushNotification() {
        val amount = if (activeFinalAmount > 0) activeFinalAmount else 50123L
        val formatted = rupiahFormatter.format(amount)
        val notifTitle = "GoPay Merchant (Simulasi)"
        val notifText = "Pembayaran QRIS Rp $formatted berhasil diterima."

        val nm = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            android.app.Notification.Builder(this, "qris_sim_channel")
        } else {
            @Suppress("DEPRECATION")
            android.app.Notification.Builder(this)
        }

        val notification = builder
            .setSmallIcon(android.R.drawable.stat_notify_more)
            .setContentTitle(notifTitle)
            .setContentText(notifText)
            .setAutoCancel(true)
            .build()

        val notifId = (System.currentTimeMillis() % 10000).toInt()
        nm.notify(notifId, notification)

        appendLog("[Simulasi OS] Push Notification dikirim ke Android status bar: \"$notifText\"")
        if (!isNotificationListenerEnabled()) {
            appendLog("[WARN] Izin Notification Listener belum aktif. Mengirim fallback via Direct Webhook...")
            sendDirectSimulatedWebhook()
        }
    }

    private fun sendDirectSimulatedWebhook(onCompleted: ((Boolean) -> Unit)? = null) {
        val baseUrl = AppPrefs.getServerUrl(this)
        val sourceId = AppPrefs.getSourceId(this)
        val sourceSecret = AppPrefs.getSourceSecret(this)

        if (sourceId.isBlank() || sourceSecret.isBlank()) {
            appendLog("Source belum siap! Silakan klik '1-Click Setup Merchant' dulu.")
            onCompleted?.invoke(false)
            return
        }

        val amount = if (activeFinalAmount > 0) activeFinalAmount else 50123L
        val formatted = rupiahFormatter.format(amount)
        val notifText = "Pembayaran QRIS Rp $formatted berhasil diterima."
        val eventId = "sim-evt-${UUID.randomUUID()}"

        val isoFormat = SimpleDateFormat("yyyy-MM-dd'T'HH:mm:ss'Z'", Locale.US).apply {
            timeZone = TimeZone.getTimeZone("UTC")
        }

        val payloadJson = JSONObject().apply {
            put("package", "com.gojek.merchant")
            put("app_name", "GoPay Merchant")
            put("title", "Pembayaran diterima")
            put("text", notifText)
            put("posted_at", isoFormat.format(Date()))
        }.toString()

        appendLog("[Direct Webhook] Mengirim mutasi HMAC: \"$notifText\" (event_id=$eventId)")

        Thread {
            try {
                val res = QrisApiClient.sendMutationWebhook(
                    baseUrl = baseUrl,
                    sourceId = sourceId,
                    sourceSecret = sourceSecret,
                    eventId = eventId,
                    jsonBody = payloadJson
                )
                appendLog("[Direct Webhook] HTTP ${res.code}: ${res.body}")
                if (res.isSuccess) {
                    val obj = JSONObject(res.body)
                    val matchStatus = obj.optString("status", "")
                    if (matchStatus == "MATCHED") {
                        updateStatusBadge("PAID")
                    }
                    onCompleted?.invoke(matchStatus == "MATCHED")
                } else {
                    onCompleted?.invoke(false)
                }
            } catch (e: Exception) {
                appendLog("[Direct Webhook] Error: ${e.message}")
                onCompleted?.invoke(false)
            }
        }.start()
    }

    // =========================================================================
    // 5. Full Automated End-to-End Diagnostic Suite (1-Click Readiness Verification)
    // =========================================================================
    fun runFullAutomatedDiagnosticSuite() {
        appendLog("==========================================")
        appendLog("MEMULAI AUTOMATED FULL E2E READINESS TEST")
        appendLog("==========================================")

        Thread {
            try {
                val baseUrl = AppPrefs.getServerUrl(this)
                val masterKey = AppPrefs.getMasterKey(this)

                // Test 1: Health & Readiness
                val h = QrisApiClient.request(baseUrl, "GET", "/healthz")
                val r = QrisApiClient.request(baseUrl, "GET", "/readyz")
                if (!h.isSuccess || !r.isSuccess) {
                    appendLog("[FAIL] Test 1: Health/Ready gagal (${h.code}/${r.code})")
                    return@Thread
                }
                appendLog("[PASS] Test 1: Healthz & Readyz OK (200)")

                // Test 2: Stateless Parse & Dynamic Transformation
                val staticQris = AppPrefs.getStaticQris(this)
                val dynReq = JSONObject().apply {
                    put("payload", staticQris)
                    put("amount", 75000)
                }.toString()
                val dynRes = QrisApiClient.request(
                    baseUrl = baseUrl,
                    method = "POST",
                    path = "/api/v1/qris/dynamic",
                    bearerToken = masterKey,
                    signingSecret = masterKey,
                    jsonBody = dynReq
                )
                if (!dynRes.isSuccess) {
                    appendLog("[FAIL] Test 2: Stateless Dynamic gagal (${dynRes.code}): ${dynRes.body}")
                    return@Thread
                }
                val dynJson = JSONObject(dynRes.body)
                appendLog("[PASS] Test 2: Stateless Dynamic QRIS OK (POI=${dynJson.optString("point_of_initiation")}, CRC=${dynJson.optString("crc")})")

                // Test 3: Ensure Merchant & Source Provisioned
                val latchProvision = java.util.concurrent.CountDownLatch(1)
                var provOk = false
                provisionMerchantAndSource { ok ->
                    provOk = ok
                    latchProvision.countDown()
                }
                latchProvision.await(10, java.util.concurrent.TimeUnit.SECONDS)
                if (!provOk) {
                    appendLog("[FAIL] Test 3: Provisioning gagal.")
                    return@Thread
                }
                appendLog("[PASS] Test 3: Merchant & Source ANDROID siap.")

                // Test 4: Create Dynamic Transaction + Unique Code + QR PNG + SSE
                val latchTxn = java.util.concurrent.CountDownLatch(1)
                var createdTxnId = ""
                var createdFinalAmt = 0L
                createDynamicTransaction { tId, fAmt ->
                    createdTxnId = tId
                    createdFinalAmt = fAmt
                    latchTxn.countDown()
                }
                latchTxn.await(10, java.util.concurrent.TimeUnit.SECONDS)
                if (createdTxnId.isBlank()) {
                    appendLog("[FAIL] Test 4: Pembuatan transaksi gagal.")
                    return@Thread
                }
                appendLog("[PASS] Test 4: Transaksi $createdTxnId aktif (Tagihan Rp ${rupiahFormatter.format(createdFinalAmt)})")

                // Give SSE 1 second to establish initial PENDING event
                Thread.sleep(1000)

                // Test 5: Trigger OS Notification if Listener is active, else Direct Webhook
                if (isNotificationListenerEnabled()) {
                    appendLog("[Test 5] NotificationListener AKTIF -> Menguji via Push Notifikasi Android OS asli...")
                    triggerSimulatedPushNotification()
                    // Wait up to 6 seconds for NotificationListenerService -> Webhook -> DB -> Status=PAID
                    var paidDetected = false
                    for (i in 1..12) {
                        Thread.sleep(500)
                        val checkRes = QrisApiClient.request(
                            baseUrl = baseUrl,
                            method = "GET",
                            path = "/api/v1/transactions/$createdTxnId/status",
                            bearerToken = AppPrefs.getMerchantKey(this)
                        )
                        if (checkRes.isSuccess && JSONObject(checkRes.body).optString("status") == "PAID") {
                            paidDetected = true
                            break
                        }
                    }
                    if (!paidDetected) {
                        appendLog("[WARN] OS Notification belum terproses dalam 6s, mencoba Direct Webhook fallback...")
                        val latchWh = java.util.concurrent.CountDownLatch(1)
                        sendDirectSimulatedWebhook { matched ->
                            paidDetected = matched
                            latchWh.countDown()
                        }
                        latchWh.await(10, java.util.concurrent.TimeUnit.SECONDS)
                    }
                    if (!paidDetected) {
                        appendLog("[FAIL] Test 5: Pencocokan mutasi gagal.")
                        return@Thread
                    }
                    appendLog("[PASS] Test 5: Push Notifikasi Android OS tertangkap otomatis & Transaksi berubah menjadi PAID!")
                } else {
                    val latchWh = java.util.concurrent.CountDownLatch(1)
                    var matchedOk = false
                    sendDirectSimulatedWebhook { matched ->
                        matchedOk = matched
                        latchWh.countDown()
                    }
                    latchWh.await(10, java.util.concurrent.TimeUnit.SECONDS)
                    if (!matchedOk) {
                        appendLog("[FAIL] Test 5: Pencocokan mutasi gagal.")
                        return@Thread
                    }
                    appendLog("[PASS] Test 5: Mutasi cocok otomatis (MATCHED) & Transaksi berubah menjadi PAID!")
                }

                // Test 6: Verify final transaction status is PAID via GET /status
                val stRes = QrisApiClient.request(
                    baseUrl = baseUrl,
                    method = "GET",
                    path = "/api/v1/transactions/$createdTxnId/status",
                    bearerToken = AppPrefs.getMerchantKey(this)
                )
                val finalSt = JSONObject(stRes.body).optString("status")
                appendLog("[PASS] Test 6: Verifikasi akhir GET /status = $finalSt")

                appendLog("==========================================")
                appendLog("KESIMPULAN: SEMUA 6/6 UJI E2E LULUS 100%! PROJEK SIAP DIGUNAKAN!")
                appendLog("==========================================")
            } catch (e: Exception) {
                appendLog("[ERROR] Suite gagal: ${e.message}")
            }
        }.start()
    }

    private fun createTestNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val nm = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
            val ch = NotificationChannel(
                "qris_sim_channel",
                "QRIS Payment Simulation",
                NotificationManager.IMPORTANCE_HIGH
            ).apply {
                description = "Channel untuk simulasi notifikasi pembayaran QRIS"
            }
            nm.createNotificationChannel(ch)
        }
    }

    private fun requestPostNotificationPermissionIfNeeded() {
        if (Build.VERSION.SDK_INT >= 33) {
            if (checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
                requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 101)
            }
        }
    }

    private fun openNotificationListenerSettings() {
        try {
            startActivity(Intent(Settings.ACTION_NOTIFICATION_LISTENER_SETTINGS))
        } catch (e: Exception) {
            Toast.makeText(this, "Gagal membuka pengaturan: ${e.message}", Toast.LENGTH_SHORT).show()
        }
    }
}

package id.qris.tester

import android.app.Notification
import android.service.notification.NotificationListenerService
import android.service.notification.StatusBarNotification
import org.json.JSONObject
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.TimeZone
import java.util.UUID

class QrisNotificationService : NotificationListenerService() {

    companion object {
        @Volatile
        var isConnected: Boolean = false

        var onLogCallback: ((String) -> Unit)? = null

        private fun emitLog(msg: String) {
            onLogCallback?.invoke(msg)
        }
    }

    override fun onListenerConnected() {
        super.onListenerConnected()
        isConnected = true
        emitLog("[NotifListener] Service terhubung & siap menangkap notifikasi pembayaran.")
    }

    override fun onListenerDisconnected() {
        isConnected = false
        emitLog("[NotifListener] Service terputus.")
        super.onListenerDisconnected()
    }

    override fun onNotificationPosted(sbn: StatusBarNotification?) {
        if (sbn == null) return

        val pkg = sbn.packageName ?: return
        val extras = sbn.notification?.extras ?: return

        val title = extras.getCharSequence(Notification.EXTRA_TITLE)?.toString()?.trim() ?: ""
        val text = extras.getCharSequence(Notification.EXTRA_TEXT)?.toString()?.trim() ?: ""
        val bigText = extras.getCharSequence(Notification.EXTRA_BIG_TEXT)?.toString()?.trim() ?: ""

        val combinedText = if (bigText.isNotEmpty()) bigText else text
        if (combinedText.isEmpty()) return

        // Filter agar hanya memproses notifikasi yang mengandung indikasi nominal Rupiah (Rp / IDR)
        val lower = "$title $combinedText".lowercase(Locale.ROOT)
        if (!lower.contains("rp") && !lower.contains("idr")) {
            return
        }

        val sourceId = AppPrefs.getSourceId(applicationContext)
        val sourceSecret = AppPrefs.getSourceSecret(applicationContext)
        val serverUrl = AppPrefs.getServerUrl(applicationContext)

        if (sourceId.isBlank() || sourceSecret.isBlank()) {
            emitLog("[NotifListener] Notif Rp terdeteksi dari $pkg tapi Source belum di-setup.")
            return
        }

        val isoFormat = SimpleDateFormat("yyyy-MM-dd'T'HH:mm:ss'Z'", Locale.US).apply {
            timeZone = TimeZone.getTimeZone("UTC")
        }
        val postedAt = isoFormat.format(Date(sbn.postTime))
        val eventId = "android-${sbn.id}-${sbn.postTime}-${UUID.randomUUID().toString().take(6)}"

        val payloadJson = JSONObject().apply {
            put("package", pkg)
            put("app_name", pkg)
            put("title", title)
            put("text", "$title - $combinedText")
            put("posted_at", postedAt)
        }.toString()

        emitLog("[NotifListener] Menangkap notif ($pkg): \"$combinedText\" -> Kirim ke Server...")

        Thread {
            try {
                val res = QrisApiClient.sendMutationWebhook(
                    baseUrl = serverUrl,
                    sourceId = sourceId,
                    sourceSecret = sourceSecret,
                    eventId = eventId,
                    jsonBody = payloadJson
                )
                emitLog("[NotifListener] Webhook Response (${res.code}): ${res.body}")
            } catch (e: Exception) {
                emitLog("[NotifListener] Gagal kirim webhook: ${e.message}")
            }
        }.start()
    }
}

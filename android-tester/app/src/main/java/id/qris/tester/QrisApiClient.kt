package id.qris.tester

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import java.io.BufferedReader
import java.io.InputStreamReader
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest
import java.util.UUID
import javax.crypto.Mac
import javax.crypto.spec.SecretKeySpec

data class HttpResult(
    val code: Int,
    val body: String,
    val headers: Map<String, String> = emptyMap()
) {
    val isSuccess: Boolean get() = code in 200..299
}

object QrisApiClient {

    fun sha256Hex(bytes: ByteArray): String {
        val md = MessageDigest.getInstance("SHA-256")
        val digest = md.digest(bytes)
        return digest.joinToString("") { "%02x".format(it) }
    }

    fun hmacSha256Hex(secret: String, data: String): String {
        val mac = Mac.getInstance("HmacSHA256")
        val keySpec = SecretKeySpec(secret.toByteArray(Charsets.UTF_8), "HmacSHA256")
        mac.init(keySpec)
        val raw = mac.doFinal(data.toByteArray(Charsets.UTF_8))
        return raw.joinToString("") { "%02x".format(it) }
    }

    /**
     * Canonical request signature (RULES §4 SR-02 / ERP §2):
     * timestamp + "\n" + nonce + "\n" + METHOD + "\n" + path_and_query + "\n" + hex(sha256(raw_body))
     */
    fun signApiRequest(
        secret: String,
        method: String,
        pathAndQuery: String,
        bodyBytes: ByteArray
    ): Triple<String, String, String> {
        val ts = (System.currentTimeMillis() / 1000L).toString()
        val nonce = UUID.randomUUID().toString().replace("-", "")
        val bodyHash = sha256Hex(bodyBytes)
        val canonical = "$ts\n$nonce\n${method.uppercase()}\n$pathAndQuery\n$bodyHash"
        val sig = "v1=${hmacSha256Hex(secret, canonical)}"
        return Triple(ts, nonce, sig)
    }

    /**
     * Canonical inbound mutation webhook signature (src/api/handlers/webhooks.rs):
     * timestamp + "\n" + event_id + "\n" + sha256(raw_body)
     */
    fun signWebhookMutation(
        sourceSecret: String,
        eventId: String,
        bodyBytes: ByteArray
    ): Pair<String, String> {
        val ts = (System.currentTimeMillis() / 1000L).toString()
        val bodyHash = sha256Hex(bodyBytes)
        val canonical = "$ts\n$eventId\n$bodyHash"
        val sig = "v1=${hmacSha256Hex(sourceSecret, canonical)}"
        return Pair(ts, sig)
    }

    fun request(
        baseUrl: String,
        method: String,
        path: String,
        bearerToken: String? = null,
        signingSecret: String? = null,
        jsonBody: String? = null,
        extraHeaders: Map<String, String> = emptyMap()
    ): HttpResult {
        val fullUrl = "${baseUrl.trimEnd('/')}$path"
        val conn = (URL(fullUrl).openConnection() as HttpURLConnection).apply {
            requestMethod = method.uppercase()
            connectTimeout = 8000
            readTimeout = 10000
            useCaches = false
        }

        val bodyBytes = jsonBody?.toByteArray(Charsets.UTF_8) ?: ByteArray(0)

        if (!bearerToken.isNullOrBlank()) {
            conn.setRequestProperty("Authorization", "Bearer $bearerToken")
        }

        if (jsonBody != null) {
            conn.setRequestProperty("Content-Type", "application/json")
        }

        // Attach signed request headers when signingSecret is provided for non-GET calls
        if (!signingSecret.isNullOrBlank() && method.uppercase() != "GET") {
            val (ts, nonce, sig) = signApiRequest(signingSecret, method, path, bodyBytes)
            conn.setRequestProperty("X-Qris-Timestamp", ts)
            conn.setRequestProperty("X-Qris-Nonce", nonce)
            conn.setRequestProperty("X-Qris-Signature", sig)
        }

        for ((k, v) in extraHeaders) {
            conn.setRequestProperty(k, v)
        }

        if (jsonBody != null && method.uppercase() != "GET") {
            conn.doOutput = true
            conn.outputStream.use { os ->
                os.write(bodyBytes)
                os.flush()
            }
        }

        val code = conn.responseCode
        val stream = if (code in 200..299) conn.inputStream else conn.errorStream
        val respText = stream?.bufferedReader(Charsets.UTF_8)?.use { it.readText() } ?: ""

        val respHeaders = mutableMapOf<String, String>()
        conn.headerFields?.forEach { (k, v) ->
            if (k != null && !v.isNullOrEmpty()) {
                respHeaders[k.lowercase()] = v.joinToString(", ")
            }
        }
        conn.disconnect()
        return HttpResult(code, respText, respHeaders)
    }

    fun sendMutationWebhook(
        baseUrl: String,
        sourceId: String,
        sourceSecret: String,
        eventId: String,
        jsonBody: String
    ): HttpResult {
        val path = "/api/v1/webhooks/mutation/$sourceId"
        val bodyBytes = jsonBody.toByteArray(Charsets.UTF_8)
        val (ts, sig) = signWebhookMutation(sourceSecret, eventId, bodyBytes)
        return request(
            baseUrl = baseUrl,
            method = "POST",
            path = path,
            jsonBody = jsonBody,
            extraHeaders = mapOf(
                "X-Qris-Timestamp" to ts,
                "X-Qris-Event-Id" to eventId,
                "X-Qris-Signature" to sig
            )
        )
    }

    fun fetchQrBitmap(
        baseUrl: String,
        txnId: String,
        bearerToken: String
    ): Bitmap? {
        val fullUrl = "${baseUrl.trimEnd('/')}/api/v1/transactions/$txnId/qr.png"
        val conn = (URL(fullUrl).openConnection() as HttpURLConnection).apply {
            requestMethod = "GET"
            connectTimeout = 8000
            readTimeout = 10000
            setRequestProperty("Authorization", "Bearer $bearerToken")
        }
        return try {
            if (conn.responseCode == 200) {
                conn.inputStream.use { BitmapFactory.decodeStream(it) }
            } else {
                null
            }
        } finally {
            conn.disconnect()
        }
    }

    fun streamTransactionEvents(
        baseUrl: String,
        txnId: String,
        bearerToken: String,
        shouldContinue: () -> Boolean,
        onEvent: (eventType: String, data: String) -> Unit
    ) {
        val fullUrl = "${baseUrl.trimEnd('/')}/api/v1/transactions/$txnId/stream"
        val conn = (URL(fullUrl).openConnection() as HttpURLConnection).apply {
            requestMethod = "GET"
            connectTimeout = 8000
            readTimeout = 65000
            setRequestProperty("Authorization", "Bearer $bearerToken")
            setRequestProperty("Accept", "text/event-stream")
        }
        try {
            if (conn.responseCode != 200) {
                val err = conn.errorStream?.bufferedReader()?.use { it.readText() } ?: ""
                onEvent("error", "HTTP ${conn.responseCode}: $err")
                return
            }
            BufferedReader(InputStreamReader(conn.inputStream, Charsets.UTF_8)).use { reader ->
                var currentEvent = "message"
                val dataBuffer = StringBuilder()

                while (shouldContinue()) {
                    val line = reader.readLine() ?: break
                    when {
                        line.startsWith(":") -> {
                            onEvent("ping", line.removePrefix(":").trim())
                        }
                        line.startsWith("event:") -> {
                            currentEvent = line.removePrefix("event:").trim()
                        }
                        line.startsWith("data:") -> {
                            if (dataBuffer.isNotEmpty()) dataBuffer.append("\n")
                            dataBuffer.append(line.removePrefix("data:").trim())
                        }
                        line.isEmpty() -> {
                            if (dataBuffer.isNotEmpty()) {
                                onEvent(currentEvent, dataBuffer.toString())
                                dataBuffer.clear()
                                currentEvent = "message"
                            }
                        }
                    }
                }
            }
        } catch (e: Exception) {
            if (shouldContinue()) {
                onEvent("stream_closed", e.message ?: "closed")
            }
        } finally {
            conn.disconnect()
        }
    }
}

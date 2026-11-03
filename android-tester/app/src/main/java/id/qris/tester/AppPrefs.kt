package id.qris.tester

import android.content.Context
import android.content.SharedPreferences

object AppPrefs {
    private const val PREFS_NAME = "qris_tester_prefs"

    const val DEFAULT_SERVER_URL = "http://127.0.0.1:8080"
    const val DEFAULT_MASTER_KEY = "test_master_key_1234567890_very_long_and_secret"
    const val DEFAULT_STATIC_QRIS =
        "00020101021126600014ID.EXAMPLE.WWW011893600000000000000102090000000010303UMI51440014ID.CO.QRIS.WWW0215ID10240012345670303UMI5204581253033605802ID5908KOS SAYA6007JAKARTA63044CDB"

    private fun prefs(ctx: Context): SharedPreferences =
        ctx.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)

    var serverUrl: String
        get() = _serverUrl
        set(value) { _serverUrl = value }
    private var _serverUrl: String = DEFAULT_SERVER_URL

    fun getServerUrl(ctx: Context): String =
        prefs(ctx).getString("server_url", DEFAULT_SERVER_URL)?.trim()?.trimEnd('/')
            ?: DEFAULT_SERVER_URL

    fun setServerUrl(ctx: Context, value: String) {
        prefs(ctx).edit().putString("server_url", value.trim().trimEnd('/')).apply()
    }

    fun getMasterKey(ctx: Context): String =
        prefs(ctx).getString("master_key", DEFAULT_MASTER_KEY)?.trim() ?: DEFAULT_MASTER_KEY

    fun setMasterKey(ctx: Context, value: String) {
        prefs(ctx).edit().putString("master_key", value.trim()).apply()
    }

    fun getStaticQris(ctx: Context): String =
        prefs(ctx).getString("static_qris", DEFAULT_STATIC_QRIS)?.trim() ?: DEFAULT_STATIC_QRIS

    fun setStaticQris(ctx: Context, value: String) {
        prefs(ctx).edit().putString("static_qris", value.trim()).apply()
    }

    fun getMerchantId(ctx: Context): String =
        prefs(ctx).getString("merchant_id", "") ?: ""

    fun getMerchantKey(ctx: Context): String =
        prefs(ctx).getString("merchant_key", "") ?: ""

    fun getSigningSecret(ctx: Context): String =
        prefs(ctx).getString("signing_secret", "") ?: ""

    fun getSourceId(ctx: Context): String =
        prefs(ctx).getString("source_id", "") ?: ""

    fun getSourceSecret(ctx: Context): String =
        prefs(ctx).getString("source_secret", "") ?: ""

    fun saveProvisioning(
        ctx: Context,
        merchantId: String,
        merchantKey: String,
        signingSecret: String,
        sourceId: String,
        sourceSecret: String
    ) {
        prefs(ctx).edit()
            .putString("merchant_id", merchantId)
            .putString("merchant_key", merchantKey)
            .putString("signing_secret", signingSecret)
            .putString("source_id", sourceId)
            .putString("source_secret", sourceSecret)
            .apply()
    }
}

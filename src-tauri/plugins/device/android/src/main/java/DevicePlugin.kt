package com.elevennorth.apexnorth.device

import android.app.Activity
import android.content.Intent
import android.media.AudioAttributes
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.Settings
import android.os.Handler
import android.os.Looper
import android.speech.tts.TextToSpeech
import android.speech.tts.UtteranceProgressListener
import android.speech.tts.Voice
import android.view.WindowManager
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import androidx.core.content.FileProvider
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.util.Locale

@InvokeArg
class SpeakArgs {
    lateinit var text: String
    var voice: String? = null
    var lang: String = "en-GB"
    var rate: Float = 1f
    var volume: Float = 1f
}

@InvokeArg
class KeepAwakeArgs {
    var on: Boolean = false
}

@InvokeArg
class UrlArgs {
    lateinit var url: String
}

/** Shares downloaded updates with the system installer (its own class, so it can't clash with another FileProvider). */
class UpdateFileProvider : FileProvider()

/**
 * The race engineer's voice on Android (the system TextToSpeech engine,
 * offline voices only), keeping the screen on while racing, and in-app
 * updates (download the new APK, hand it to the system installer). Called
 * from the app's Rust code (`NativeSpeech` in lib.rs, `updater.rs`), never
 * from JavaScript.
 */
@TauriPlugin
class DevicePlugin(private val activity: Activity) : Plugin(activity), TextToSpeech.OnInitListener {
    private val lock = Any()
    private var tts: TextToSpeech? = null
    private var ready = false
    private var initError: String? = null
    /** `prepare` calls waiting for the engine to start. */
    private val waiting = mutableListOf<Invoke>()
    /** The utterance being spoken, until the engine says it's done. */
    @Volatile private var current: String? = null
    private var nextId = 0L
    /** The voice last chosen for a (voice id, language) request. */
    private var chosen: Pair<String, Voice?>? = null
    private val main = Handler(Looper.getMainLooper())

    /** The update download: "idle", "downloading", "done" or "failed". */
    @Volatile private var dlState = "idle"
    @Volatile private var dlDone = 0L
    @Volatile private var dlTotal = -1L
    @Volatile private var dlError: String? = null
    private val updateApk get() = File(File(activity.cacheDir, "updates"), "ApexNorth.apk")

    private val progress = object : UtteranceProgressListener() {
        override fun onStart(utteranceId: String?) {}
        override fun onDone(utteranceId: String?) = finished(utteranceId)
        @Deprecated("Deprecated in Java")
        override fun onError(utteranceId: String?) = finished(utteranceId)
        override fun onError(utteranceId: String?, errorCode: Int) = finished(utteranceId)
        override fun onStop(utteranceId: String?, interrupted: Boolean) = finished(utteranceId)
    }

    private fun finished(id: String?) {
        synchronized(lock) {
            if (id != null && id == current) current = null
        }
    }

    override fun onInit(status: Int) {
        val pending = synchronized(lock) {
            val engine = tts
            if (status == TextToSpeech.SUCCESS && engine != null) {
                ready = true
                engine.setAudioAttributes(
                    AudioAttributes.Builder()
                        .setUsage(AudioAttributes.USAGE_ASSISTANCE_NAVIGATION_GUIDANCE)
                        .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
                        .build()
                )
                engine.setOnUtteranceProgressListener(progress)
            } else {
                initError = "This phone has no text-to-speech engine. Install Speech Services by Google from the Play Store, then try again."
                engine?.shutdown()
                tts = null
            }
            val p = waiting.toList()
            waiting.clear()
            p
        }
        pending.forEach { answer(it) }
    }

    private fun answer(invoke: Invoke) {
        val error = synchronized(lock) { if (ready) null else initError ?: "The phone's voice did not start." }
        if (error == null) invoke.resolve() else invoke.reject(error)
    }

    /** Start the engine if needed; answers once it's ready (or failed). */
    @Command
    fun prepare(invoke: Invoke) {
        val wait = synchronized(lock) {
            if (ready) {
                false
            } else {
                waiting.add(invoke)
                if (tts == null) {
                    initError = null
                    tts = TextToSpeech(activity.applicationContext, this)
                }
                true
            }
        }
        if (!wait) {
            invoke.resolve()
            return
        }
        // Never leave the caller waiting on an engine that doesn't answer.
        main.postDelayed({
            val late = synchronized(lock) { waiting.remove(invoke) }
            if (late) invoke.reject("The phone's voice took too long to start.")
        }, 15_000)
    }

    @Command
    fun speak(invoke: Invoke) {
        val args = invoke.parseArgs(SpeakArgs::class.java)
        synchronized(lock) {
            val engine = tts
            if (engine == null || !ready) {
                invoke.reject(initError ?: "The phone's voice is still starting.")
                return
            }
            pickVoice(engine, args.voice, args.lang)?.let { engine.voice = it }
            engine.setSpeechRate(args.rate)
            val id = "apexnorth-${nextId++}"
            current = id
            val params = Bundle()
            params.putFloat(TextToSpeech.Engine.KEY_PARAM_VOLUME, args.volume.coerceIn(0f, 1f))
            if (engine.speak(args.text, TextToSpeech.QUEUE_FLUSH, params, id) != TextToSpeech.SUCCESS) {
                current = null
                invoke.reject("The phone's voice could not speak.")
                return
            }
        }
        invoke.resolve()
    }

    @Command
    fun status(invoke: Invoke) {
        val ret = JSObject()
        ret.put("speaking", current != null)
        invoke.resolve(ret)
    }

    @Command
    fun stop(invoke: Invoke) {
        synchronized(lock) {
            current = null
            tts?.stop()
        }
        invoke.resolve()
    }

    /** English voices that are installed and work without a network. */
    @Command
    fun voices(invoke: Invoke) {
        val list = JSArray()
        synchronized(lock) {
            offlineEnglish(tts).forEach { v ->
                val o = JSObject()
                o.put("id", v.name)
                o.put("name", label(v))
                o.put("lang", v.locale.toLanguageTag())
                o.put("quality", v.quality)
                list.put(o)
            }
        }
        val ret = JSObject()
        ret.put("voices", list)
        invoke.resolve(ret)
    }

    @Command
    fun keepAwake(invoke: Invoke) {
        val args = invoke.parseArgs(KeepAwakeArgs::class.java)
        activity.runOnUiThread {
            if (args.on) {
                activity.window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
            } else {
                activity.window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
            }
        }
        invoke.resolve()
    }

    private fun offlineEnglish(engine: TextToSpeech?): List<Voice> =
        (try { engine?.voices } catch (e: Exception) { null } ?: emptySet())
            .filter { it.locale.language == "en" && !it.isNetworkConnectionRequired }
            .filter { !it.features.contains(TextToSpeech.Engine.KEY_FEATURE_NOT_INSTALLED) }
            .sortedWith(compareBy<Voice>({ it.locale.toLanguageTag() }, { -it.quality }, { it.name }))

    /** The requested voice, else the best offline voice for the language. */
    private fun pickVoice(engine: TextToSpeech, id: String?, lang: String): Voice? {
        val key = "${id ?: ""}|$lang"
        chosen?.let { if (it.first == key) return it.second }
        val voices = offlineEnglish(engine)
        val wanted = Locale.forLanguageTag(lang)
        val pick = voices.firstOrNull { it.name == id }
            ?: voices.filter { it.locale.country == wanted.country }.maxByOrNull { it.quality }
            ?: voices.maxByOrNull { it.quality }
        chosen = Pair(key, pick)
        return pick
    }

    private fun label(v: Voice): String {
        // Names look like "en-gb-x-gbd-local"; the middle code tells voices apart.
        val code = v.name.split("-").getOrNull(3)?.uppercase()
        val country = v.locale.getDisplayCountry(Locale.ENGLISH).ifEmpty { v.locale.toLanguageTag() }
        return if (code != null) "$country · $code" else "$country · ${v.name}"
    }

    /** GET a small text file (the update feed), following redirects. */
    @Command
    fun fetchText(invoke: Invoke) {
        val args = invoke.parseArgs(UrlArgs::class.java)
        Thread {
            try {
                val conn = open(args.url)
                val text = conn.inputStream.bufferedReader().use { it.readText() }
                val ret = JSObject()
                ret.put("text", text)
                invoke.resolve(ret)
            } catch (e: Exception) {
                invoke.reject("Couldn't reach the update server: ${e.message}")
            }
        }.start()
    }

    /** Start downloading the update APK; `updateProgress` reports how it's going. */
    @Command
    fun downloadUpdate(invoke: Invoke) {
        val args = invoke.parseArgs(UrlArgs::class.java)
        if (dlState == "downloading") {
            invoke.resolve()
            return
        }
        dlState = "downloading"
        dlDone = 0
        dlTotal = -1
        dlError = null
        Thread {
            try {
                val file = updateApk
                file.parentFile?.mkdirs()
                val part = File(file.path + ".part")
                val conn = open(args.url)
                dlTotal = conn.contentLengthLong
                conn.inputStream.use { input ->
                    part.outputStream().use { out ->
                        val buf = ByteArray(64 * 1024)
                        while (true) {
                            val n = input.read(buf)
                            if (n < 0) break
                            out.write(buf, 0, n)
                            dlDone += n
                        }
                    }
                }
                if (!part.renameTo(file)) throw Exception("couldn't save the download")
                dlState = "done"
            } catch (e: Exception) {
                dlError = "Download failed: ${e.message}"
                dlState = "failed"
            }
        }.start()
        invoke.resolve()
    }

    @Command
    fun updateProgress(invoke: Invoke) {
        val ret = JSObject()
        ret.put("state", dlState)
        ret.put("done", dlDone)
        ret.put("total", dlTotal)
        ret.put("error", dlError)
        invoke.resolve(ret)
    }

    /**
     * Open the system installer on the downloaded APK. Android first asks
     * the user to let ApexNorth install apps: then this opens that setting
     * and answers `needsPermission`, and the user taps Update again.
     */
    @Command
    fun installUpdate(invoke: Invoke) {
        val ret = JSObject()
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O && !activity.packageManager.canRequestPackageInstalls()) {
                activity.startActivity(
                    Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:${activity.packageName}"))
                )
                ret.put("needsPermission", true)
                invoke.resolve(ret)
                return
            }
            val uri = FileProvider.getUriForFile(activity, "${activity.packageName}.apexnorth.updates", updateApk)
            val intent = Intent(Intent.ACTION_VIEW)
                .setDataAndType(uri, "application/vnd.android.package-archive")
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
            activity.startActivity(intent)
            ret.put("needsPermission", false)
            invoke.resolve(ret)
        } catch (e: Exception) {
            invoke.reject("Couldn't open the installer: ${e.message}")
        }
    }

    private fun open(url: String): HttpURLConnection {
        val conn = URL(url).openConnection() as HttpURLConnection
        conn.connectTimeout = 15_000
        conn.readTimeout = 30_000
        conn.instanceFollowRedirects = true
        conn.setRequestProperty("User-Agent", "ApexNorth")
        if (conn.responseCode !in 200..299) throw Exception("HTTP ${conn.responseCode}")
        return conn
    }
}

import AVFoundation
import SwiftRs
import Tauri
import UIKit
import WebKit

struct SpeakArgs: Decodable {
  let text: String
  let voice: String?
  let lang: String?
  let rate: Float?
  let volume: Float?
}

struct KeepAwakeArgs: Decodable {
  let on: Bool
}

/// The race engineer's voice on iOS (AVSpeechSynthesizer, on-device voices)
/// and keeping the screen on while racing. Called from the app's Rust code
/// (`NativeSpeech` in lib.rs), never from JavaScript.
class DevicePlugin: Plugin, AVSpeechSynthesizerDelegate {
  private let synth = AVSpeechSynthesizer()
  private let lock = NSLock()
  /// The utterance being spoken, until the synthesizer says it's done.
  private var current: AVSpeechUtterance?
  /// The voice last chosen for a (voice id, language) request.
  private var chosen: (key: String, voice: AVSpeechSynthesisVoice?)?

  override init() {
    super.init()
    synth.delegate = self
  }

  @objc public func prepare(_ invoke: Invoke) throws {
    invoke.resolve()
  }

  @objc public func speak(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(SpeakArgs.self)
    let utterance = AVSpeechUtterance(string: args.text)
    utterance.voice = pickVoice(id: args.voice, lang: args.lang ?? "en-GB")
    let rate = AVSpeechUtteranceDefaultSpeechRate * (args.rate ?? 1)
    utterance.rate = min(max(rate, AVSpeechUtteranceMinimumSpeechRate), AVSpeechUtteranceMaximumSpeechRate)
    utterance.volume = min(max(args.volume ?? 1, 0), 1)
    lock.lock()
    current = utterance
    lock.unlock()
    activateAudio()
    if synth.isSpeaking {
      synth.stopSpeaking(at: .immediate)
    }
    synth.speak(utterance)
    invoke.resolve()
  }

  @objc public func status(_ invoke: Invoke) throws {
    lock.lock()
    let speaking = current != nil
    lock.unlock()
    let ret: JsonObject = ["speaking": speaking]
    invoke.resolve(ret)
  }

  @objc public func stop(_ invoke: Invoke) throws {
    lock.lock()
    current = nil
    lock.unlock()
    synth.stopSpeaking(at: .immediate)
    invoke.resolve()
  }

  /// English voices installed on the device.
  @objc public func voices(_ invoke: Invoke) throws {
    let list: [[String: Any]] = englishVoices().map { v in
      ["id": v.identifier, "name": "\(v.name) (\(v.language))", "lang": v.language, "quality": v.quality.rawValue]
    }
    let ret: JsonObject = ["voices": list]
    invoke.resolve(ret)
  }

  @objc public func keepAwake(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(KeepAwakeArgs.self)
    DispatchQueue.main.async {
      UIApplication.shared.isIdleTimerDisabled = args.on
    }
    invoke.resolve()
  }

  // MARK: - AVSpeechSynthesizerDelegate

  func speechSynthesizer(_ synthesizer: AVSpeechSynthesizer, didFinish utterance: AVSpeechUtterance) {
    finished(utterance)
  }

  func speechSynthesizer(_ synthesizer: AVSpeechSynthesizer, didCancel utterance: AVSpeechUtterance) {
    finished(utterance)
  }

  private func finished(_ utterance: AVSpeechUtterance) {
    lock.lock()
    let done = current === utterance
    if done { current = nil }
    lock.unlock()
    if done {
      // Let music the driver is playing come back up.
      try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
    }
  }

  /// Speak over other audio (ducking it), even with the silent switch on.
  private func activateAudio() {
    let session = AVAudioSession.sharedInstance()
    try? session.setCategory(.playback, mode: .voicePrompt, options: [.duckOthers, .interruptSpokenAudioAndMixWithOthers])
    try? session.setActive(true)
  }

  private func englishVoices() -> [AVSpeechSynthesisVoice] {
    AVSpeechSynthesisVoice.speechVoices()
      .filter { $0.language.hasPrefix("en") }
      .sorted { ($0.language, -$0.quality.rawValue, $0.name) < ($1.language, -$1.quality.rawValue, $1.name) }
  }

  /// The requested voice, else the best installed voice for the language.
  private func pickVoice(id: String?, lang: String) -> AVSpeechSynthesisVoice? {
    let key = "\(id ?? "")|\(lang)"
    if let c = chosen, c.key == key { return c.voice }
    var pick: AVSpeechSynthesisVoice? = nil
    if let id = id, !id.isEmpty { pick = AVSpeechSynthesisVoice(identifier: id) }
    if pick == nil {
      pick = englishVoices().filter { $0.language == lang }.max { $0.quality.rawValue < $1.quality.rawValue }
    }
    if pick == nil { pick = AVSpeechSynthesisVoice(language: lang) }
    chosen = (key, pick)
    return pick
  }
}

@_cdecl("init_plugin_apexnorth_device")
func initPlugin() -> Plugin {
  return DevicePlugin()
}

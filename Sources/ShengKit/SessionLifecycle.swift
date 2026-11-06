import Foundation

/// Starting, stopping and reconnecting a captioning session, and polling the engine while one runs.
extension SessionModel {
    func startPolling() {
        guard timer == nil else { return }
        timer = Timer.scheduledTimer(withTimeInterval: 0.1, repeats: true) { [weak self] _ in Task { @MainActor in self?.poll() } }
    }
    func stopPolling() { timer?.invalidate(); timer = nil }
    public func start(automatic: Bool = false) async {
        lifecycleLog.notice("start_requested active=\(self.active, privacy: .public)")
        guard !active else { return }
        guard let speech = profiles.first(where: { $0.id == selectedProfile }) else {
            error = L10n.text("请先选择并保存一个服务配置。"); return
        }
        let profile = effective(speech)
        if !automatic { retryAttempt = 0 }
        captureIntent = true
        startPolling()
        epoch += 1; let token = epoch; isBusy = true; error = nil; failureCode = nil
        do {
            try profile.requireSupported(source: sourceLanguage, target: targetLanguage)
            if profile.provider == "appleSpeech" { _ = try await AppleResources.perform(kind: 3, source: sourceLanguage, target: "original") }
            if profile.usesIndependentText, profile.textProvider == "apple", targetLanguage != "original" { _ = try await AppleResources.perform(kind: 4, source: sourceLanguage, target: targetLanguage) }
            guard token == epoch else { return }
            lifecycleLog.notice("start_stage=lease")
            try lease.acquire()
            _ = try engine.drain()
            if !isPaused && !automatic { presentation = CaptionPresentation() }
            lifecycleLog.notice("start_stage=engine")
            try engine.start(request: profile.request(input: input, source: sourceLanguage, target: targetLanguage, secrets: effectiveCredentials(for: speech)))
            let deadline = Date().addingTimeInterval(35)
            while Date() < deadline {
                guard token == epoch else { return }
                let state = try engine.snapshot()
                if state.status == "failed" { throw EngineError(code: state.error ?? "provider_connection_failed") }
                if state.status == "listening" { break }
                try await Task.sleep(nanoseconds: 100_000_000)
            }
            let state = try engine.snapshot()
            lifecycleLog.notice("start_stage=provider_ready")
            guard state.status == "listening" else { throw EngineError(code: "provider_connection_failed") }
            if archive == nil && (saveHistory || recordAudio) {
                archive = try SessionArchive(store: store, rate: state.sampleRate, input: input, history: saveHistory, recording: recordAudio, sourceLanguage: sourceLanguage, targetLanguage: targetLanguage) { [weak self] in
                    Task { @MainActor in self?.error = L10n.text("保存失败或磁盘写入落后，会话已停止。"); await self?.stop() }
                }
            }
            activeSessionID = archive?.directory.lastPathComponent
            let writer = archive
            let engine = self.engine
            let health = healthTracker
            health.begin(input: input)
            let capture = AudioCapture(handler: { [weak self] source, data in
                health.receive(data, source: source)
                if engine.push(data, source: source) { writer?.append(data, source: source) }
                else { Task { @MainActor in self?.error = L10n.text("音频处理未能跟上，会话已停止。"); await self?.stop() } }
            }, failed: { [weak self] in health.fail(); Task { @MainActor in self?.captureHealth = health.snapshot(); self?.error = L10n.text("音频采集中断，请检查权限和设备。"); await self?.stop() } })
            self.capture = capture
            lifecycleLog.notice("start_stage=capture")
            try await capture.start(input: input, sampleRate: state.sampleRate, microphoneUID: microphoneUID, systemProcessID: systemProcessID == 0 ? nil : systemProcessID)
            guard token == epoch else { await capture.stop(); return }
            // Permission dialogs are not part of the silence/no-PCM observation window.
            health.begin(input: input)
            isCapturing = true; isPaused = false; isBusy = false; lifecycleLog.notice("start_stage=listening"); onCaptureStarted?()
        } catch {
            lifecycleLog.error("start_stage=failed code=\((error as? EngineError)?.code ?? "capture_failed", privacy: .public)")
            if let failure = error as? EngineError {
                await handleFailure(code: failure.code)
            } else {
                let failure = CaptureFailure.classify(error)
                await stop()
                healthTracker.fail(); captureHealth = healthTracker.snapshot()
                self.failureCode = failure.code
                self.error = failure.localizedDescription
            }
        }
    }
    public func stop(pause: Bool = false, recovering: Bool = false) async {
        if !recovering { captureIntent = false; recoveryTask?.cancel(); recoveryTask = nil; isReconnecting = false }
        guard !isStopping else { return }
        isStopping = true; epoch += 1; isBusy = true; isCapturing = false
        await capture?.stop(); capture = nil; engine.stop()
        let deadline = Date().addingTimeInterval(12)
        while Date() < deadline {
            if (try? engine.snapshot().settled) == true { break }
            try? await Task.sleep(nanoseconds: 100_000_000)
        }
        poll()
        stopPolling()
        if !pause { await archive?.close(); archive = nil; activeSessionID = nil; recordAudio = false }
        lease.release(); isPaused = pause; isBusy = false; isStopping = false
    }
    /// The current pair turned around; nil when either side cannot be swapped (automatic input, original only).
    public var swappedLanguages: LanguagePair? {
        guard sourceLanguage != "auto", targetLanguage != "original", sourceLanguage != targetLanguage else { return nil }
        return LanguagePair(source: targetLanguage, target: sourceLanguage)
    }
    public func switchDirection() async {
        guard let pair = swappedLanguages else { return }
        await reconfigure(profileID: selectedProfile, source: pair.source, target: pair.target)
    }
    public func reconfigure(profileID: UUID, source: String, target: String) async {
        guard !isBusy, !isReconnecting, let speech = profiles.first(where: { $0.id == profileID }) else { return }
        let profile = effective(speech)
        let wasCapturing = isCapturing, token = epoch
        isBusy = true
        do {
            try profile.requireSupported(source: source, target: target)
            try engine.validate(request: profile.request(input: input, source: source, target: target, secrets: effectiveCredentials(for: speech)))
            if wasCapturing, profile.provider == "appleSpeech" { _ = try await AppleResources.perform(kind: 3, source: source, target: "original") }
            if wasCapturing, profile.usesIndependentText, profile.textProvider == "apple", target != "original" { _ = try await AppleResources.perform(kind: 4, source: source, target: target) }
        } catch {
            if token == epoch { self.error = error.localizedDescription; isBusy = false }
            return
        }
        guard token == epoch else { return }
        isBusy = false
        if wasCapturing { await stop(pause: true) }
        selectedProfile = profileID; sourceLanguage = source; targetLanguage = target; savePreferences()
        if wasCapturing { await start() }
    }
    public func poll() {
        do {
            if isCapturing { publishHealth() }
            let current = try engine.snapshot()
            if current.revision != snapshot?.revision { snapshot = current }
            if isCapturing, let echo = current.echo, Date().timeIntervalSince(lastEchoReport) >= 5 {
                lastEchoReport = Date()
                lifecycleLog.notice("echo frames=\(echo.frames, privacy: .public) matched=\(echo.matchedReferences, privacy: .public) missing=\(echo.missingReferences, privacy: .public) resets=\(echo.resets, privacy: .public) inputRms=\(echo.inputRms ?? -1, privacy: .public) outputRms=\(echo.outputRms ?? -1, privacy: .public) referenceRms=\(echo.referenceRms ?? -1, privacy: .public) reductionDb=\(echo.reductionDb ?? -1, privacy: .public)")
            }
            for caption in try engine.drain() { presentation.append(caption); archive?.append(caption); onCaption?(caption) }
            if current.status == "failed", !isStopping, isCapturing, !isReconnecting {
                Task { await handleFailure(code: current.error ?? "provider_request_failed") }
            }
        } catch { self.error = L10n.text("字幕引擎状态读取失败。") }
    }
    func publishHealth() {
        let next = healthTracker.snapshot()
        guard CaptureHealthSnapshot.needsPublish(previous: captureHealth, next: next, sinceLastPublish: Date().timeIntervalSince(lastHealthPublish)) else { return }
        captureHealth = next; lastHealthPublish = Date()
    }
    func handleFailure(code: String) async {
        failureCode = code
        healthTracker.fail(); captureHealth = healthTracker.snapshot()
        let retryable = EngineError(code: code).isRetryable
        let delay = Engine.retryDelay(attempt: retryAttempt)
        guard captureIntent, retryable, delay >= 0 else {
            error = EngineError(code: code).localizedDescription
            await stop(); return
        }
        retryAttempt += 1
        await stop(pause: true, recovering: true)
        guard captureIntent else { return }
        isReconnecting = true; isPaused = false; error = nil
        recoveryTask = Task { [weak self] in
            do { try await Task.sleep(nanoseconds: UInt64(delay) * 1_000_000) } catch { return }
            guard let self, self.captureIntent, !Task.isCancelled else { return }
            self.isReconnecting = false
            await self.start(automatic: true)
        }
    }
}

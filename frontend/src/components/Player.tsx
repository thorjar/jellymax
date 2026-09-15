import { useEffect, useRef, useState, type RefObject } from "react";
import { api, getToken, resolveUrl } from "../api/client";
import type { Item, MediaSource, MediaStream } from "../api/types";
import { TICKS_PER_SECOND, formatSeconds, ticksToSeconds } from "../lib/format";
import { Spinner } from "./Spinner";
import { PlaybackIcon } from "./PlaybackIcon";
import { browserSupportsHevc, browserSupportsMkv, buildBrowserDeviceProfile, safariCanDirectPlay } from "../lib/browserDeviceProfile";
import { parseWebVtt, visibleSubtitle, type SubtitleCue } from "../lib/subtitles";
import { toWebVtt } from "../lib/subtitles";
import { deleteSubtitle, listSavedSubtitles, saveSubtitle, type SavedSubtitle } from "../lib/subtitleLibrary";
import type { SubtitleSearchResult } from "../api/types";

interface PlayerProps { item: Item; }

export function Player({ item }: PlayerProps) {
  const [source, setSource] = useState<MediaSource | null>(null);
  const [playbackUrl, setPlaybackUrl] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [playbackNotice, setPlaybackNotice] = useState<string | null>(null);
  const [currentTime, setCurrentTime] = useState(0);
  const [duration, setDuration] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [volume, setVolume] = useState(1);
  const [muted, setMuted] = useState(false);
  const [fullscreen, setFullscreen] = useState(false);
  const [controlsVisible, setControlsVisible] = useState(true);
  const [subtitleMenuOpen, setSubtitleMenuOpen] = useState(false);
  const [audioMenuOpen, setAudioMenuOpen] = useState(false);
  const [selectedAudio, setSelectedAudio] = useState<number | null>(null);
  const [selectedSubtitle, setSelectedSubtitle] = useState<number | string | null>(null);
  const [savedSubtitles, setSavedSubtitles] = useState<SavedSubtitle[]>([]);
  const [subtitleSearchOpen, setSubtitleSearchOpen] = useState(false);
  const [subtitleLanguage, setSubtitleLanguage] = useState("en");
  const [subtitleResults, setSubtitleResults] = useState<SubtitleSearchResult[]>([]);
  const [subtitleSearchLoading, setSubtitleSearchLoading] = useState(false);
  const [subtitleSearchError, setSubtitleSearchError] = useState<string | null>(null);
  const [subtitleUrl, setSubtitleUrl] = useState<string | null>(null);
  const [subtitleError, setSubtitleError] = useState<string | null>(null);
  const [subtitleLoading, setSubtitleLoading] = useState(false);
  const [subtitleTrackVersion, setSubtitleTrackVersion] = useState(0);
  const [subtitleRetry, setSubtitleRetry] = useState(0);
  const [subtitleWindowStart, setSubtitleWindowStart] = useState(0);
  const [captionText, setCaptionText] = useState("");
  const [pictureInPicture, setPictureInPicture] = useState(false);
  const mediaRef = useRef<HTMLMediaElement | null>(null);
  const subtitleTrackRef = useRef<HTMLTrackElement | null>(null);
  const subtitleCuesRef = useRef<SubtitleCue[]>([]);
  const loadedSubtitleIndexRef = useRef<number | string | null>(null);
  const subtitleFileInputRef = useRef<HTMLInputElement | null>(null);
  const frameRef = useRef<HTMLDivElement | null>(null);
  const controlsTimer = useRef<number | undefined>(undefined);
  const positionRef = useRef(0);
  const switchPositionRef = useRef<number | null>(null);
  const lastReportedRef = useRef(0);
  const stoppedRef = useRef(true);
  const playSessionRef = useRef<string | undefined>(undefined);
  const isAudio = item.MediaType === "Audio";

  useEffect(() => () => window.clearTimeout(controlsTimer.current), []);

  useEffect(() => {
    let cancelled = false;
    void listSavedSubtitles(item.Id).then((list) => { if (!cancelled) setSavedSubtitles(list); })
      .catch(() => { if (!cancelled) setSavedSubtitles([]); });
    return () => { cancelled = true; };
  }, [item.Id]);

  useEffect(() => {
    let cancelled = false;
    stoppedRef.current = false;
    playSessionRef.current = undefined;
    lastReportedRef.current = 0;
    positionRef.current = item.UserData?.PlaybackPositionTicks ?? 0;
    setSource(null);
    setPlaybackUrl(null);
    setError(null);
    setPlaybackNotice(null);
    setSelectedSubtitle(null);
    setSelectedAudio(null);
    setAudioMenuOpen(false);
    switchPositionRef.current = null;
    setSubtitleSearchOpen(false);
    setSubtitleResults([]);
    setSubtitleUrl(null);
    setSubtitleError(null);
    setCaptionText("");
    subtitleCuesRef.current = [];
    loadedSubtitleIndexRef.current = null;
    setPlaying(false);
    const supportsHevc = browserSupportsHevc();
    const audioProbe = document.createElement("audio");
    const supportsAc3 = audioProbe.canPlayType('audio/mp4; codecs="ac-3"') !== "";
    const supportsEac3 = audioProbe.canPlayType('audio/mp4; codecs="ec-3"') !== "";
    const resumeTicks = item.UserData?.Played ? 0 : (item.UserData?.PlaybackPositionTicks ?? 0);
    void api.playbackInfo(item.Id, supportsHevc, browserSupportsMkv(), supportsAc3, supportsEac3, buildBrowserDeviceProfile(supportsHevc), resumeTicks).then((info) => {
      if (cancelled) {
        void api.reportStopped(item.Id, positionRef.current, info.PlaySessionId).catch(() => {});
        return;
      }
      playSessionRef.current = info.PlaySessionId;
      const mediaSource = info.MediaSources?.[0];
      if (!mediaSource) { setError("The server returned no playable media source."); return; }
      const codec = mediaSource.MediaStreams.find((stream) => stream.Type === "Audio")?.Codec?.toLowerCase();
      const mime = codec === "eac3" ? 'audio/mp4; codecs="ec-3"'
        : codec === "ac3" ? 'audio/mp4; codecs="ac-3"' : null;
      const supportsAudio = mime ? document.createElement("audio").canPlayType(mime) !== ""
        : !["dts", "truehd"].includes(codec ?? "");
      const safariDirect = !isAudio && navigator.vendor.includes("Apple")
        ? safariCanDirectPlay(item.Container, mediaSource.MediaStreams)
        : null;
      const selected = safariDirect === true
        ? mediaSource.DirectStreamUrl
        : safariDirect === false && mediaSource.SupportsDirectPlay
          ? (mediaSource.FallbackTranscodingUrl ?? mediaSource.DirectStreamUrl)
        : !supportsAudio && mediaSource.AudioTranscodingUrl
        ? mediaSource.AudioTranscodingUrl
        : !mediaSource.SupportsDirectPlay && mediaSource.TranscodingUrl
          ? mediaSource.TranscodingUrl
          : mediaSource.DirectStreamUrl;
      setSource(mediaSource);
      setSelectedAudio(mediaSource.MediaStreams.find((stream) => stream.Type === "Audio")?.Index ?? null);
      setPlaybackUrl(selected);
    }).catch((reason) => {
      if (!cancelled) setError(reason instanceof Error ? reason.message : String(reason));
    });
    return () => { cancelled = true; };
  }, [item.Id, isAudio]);

  useEffect(() => {
    const onFullscreenChange = () => setFullscreen(document.fullscreenElement === frameRef.current);
    document.addEventListener("fullscreenchange", onFullscreenChange);
    return () => document.removeEventListener("fullscreenchange", onFullscreenChange);
  }, []);

  useEffect(() => {
    const video = mediaRef.current;
    if (!(video instanceof HTMLVideoElement)) return;
    const update = () => setPictureInPicture(document.pictureInPictureElement === video ||
      (video as HTMLVideoElement & { webkitPresentationMode?: string }).webkitPresentationMode === "picture-in-picture");
    video.addEventListener("enterpictureinpicture", update);
    video.addEventListener("leavepictureinpicture", update);
    video.addEventListener("webkitpresentationmodechanged", update);
    return () => {
      video.removeEventListener("enterpictureinpicture", update);
      video.removeEventListener("leavepictureinpicture", update);
      video.removeEventListener("webkitpresentationmodechanged", update);
    };
  }, [source]);

  useEffect(() => {
    const video = mediaRef.current;
    if (!(video instanceof HTMLVideoElement)) return;
    // WebKit can add native controls through its media context menu. Keep the
    // custom video player in control even if the attribute is changed later.
    const hideNativeControls = () => { if (video.controls) video.controls = false; };
    hideNativeControls();
    const observer = new MutationObserver(hideNativeControls);
    observer.observe(video, { attributes: true, attributeFilter: ["controls"] });
    return () => observer.disconnect();
  }, [source, playbackUrl]);

  useEffect(() => {
    if (selectedSubtitle === null || !source) {
      subtitleCuesRef.current = [];
      loadedSubtitleIndexRef.current = null;
      setSubtitleUrl(null);
      setSubtitleLoading(false);
      setSubtitleError(null);
      return;
    }
    if (typeof selectedSubtitle === "string") {
      const saved = savedSubtitles.find((candidate) => candidate.id === selectedSubtitle);
      if (!saved) return;
      subtitleCuesRef.current = parseWebVtt(saved.vtt);
      loadedSubtitleIndexRef.current = selectedSubtitle;
      const url = URL.createObjectURL(new Blob([saved.vtt], { type: "text/vtt" }));
      setSubtitleUrl(url);
      setSubtitleError(null);
      setSubtitleLoading(false);
      return () => URL.revokeObjectURL(url);
    }
    const stream = source.MediaStreams.find((candidate) => candidate.Type === "Subtitle" && candidate.Index === selectedSubtitle);
    if (!stream?.DeliveryUrl) return;
    const controller = new AbortController();
    const blobUrls: string[] = [];
    if (loadedSubtitleIndexRef.current !== selectedSubtitle) {
      setSubtitleUrl(null);
      subtitleCuesRef.current = [];
    }
    setSubtitleError(null);
    setSubtitleLoading(true);
    const token = getToken();
    const subtitleAddress = new URL(resolveUrl(stream.DeliveryUrl), window.location.href);
    // FFmpeg seeks to the previous container keyframe. Start before the
    // current window so a cue already in progress survives resume/seek.
    if (!stream.IsExternal && item.IsRemote) subtitleAddress.searchParams.set("StartSeconds", String(Math.max(0, subtitleWindowStart - 15)));
    const load = async () => {
      const response = await fetch(subtitleAddress.toString(), {
        headers: token ? { "X-Emby-Token": token } : undefined,
        signal: controller.signal,
      });
      if (!response.ok) {
        let reason = `Subtitle request failed (${response.status})`;
        try {
          const body = await response.json() as { Error?: string };
          if (body.Error) reason = body.Error;
        } catch { /* Non-JSON error response. */ }
        throw new Error(`${reason}. Choose the track to retry.`);
      }
      let text = "";
      let published = 0;
      const publish = (complete: string) => {
        if (!complete.trimStart().startsWith("WEBVTT")) throw new Error("The subtitle track is not valid WebVTT");
        const fresh = parseWebVtt(complete);
        if (fresh.length <= published || controller.signal.aborted) return;
        published = fresh.length;
        subtitleCuesRef.current = loadedSubtitleIndexRef.current === selectedSubtitle
          ? [...subtitleCuesRef.current.filter((cue) => cue.end > subtitleWindowStart && !fresh.some((next) => next.start === cue.start && next.end === cue.end && next.text === cue.text)), ...fresh]
          : fresh;
        loadedSubtitleIndexRef.current = selectedSubtitle;
        const url = URL.createObjectURL(new Blob([complete], { type: "text/vtt" }));
        blobUrls.push(url);
        setSubtitleUrl(url);
        setSubtitleLoading(false);
        if (!pictureInPicture) setCaptionText(visibleSubtitle(subtitleCuesRef.current, mediaRef.current?.currentTime ?? 0));
      };
      if (response.body) {
        const reader = response.body.getReader();
        const decoder = new TextDecoder();
        while (true) {
          const { done, value } = await reader.read();
          if (done) break;
          text += decoder.decode(value, { stream: true });
          const normalized = text.replace(/\r\n?/g, "\n");
          const completeEnd = normalized.lastIndexOf("\n\n");
          if (completeEnd >= 0) publish(normalized.slice(0, completeEnd + 2));
        }
        text += decoder.decode();
      } else text = await response.text();
      if (controller.signal.aborted) return;
      publish(text.replace(/\r\n?/g, "\n"));
      if (published === 0 && (stream.IsExternal || !item.IsRemote))
        throw new Error("This subtitle track has no readable captions. Choose another track.");
    };
    void load().catch((reason: unknown) => {
      if (!controller.signal.aborted) {
        console.warn("Subtitle load failed", reason);
        setSubtitleError(reason instanceof Error ? reason.message : "Subtitles could not load. Choose the track to retry.");
      }
    }).finally(() => { if (!controller.signal.aborted) setSubtitleLoading(false); });
    return () => {
      controller.abort();
      for (const url of blobUrls) URL.revokeObjectURL(url);
    };
  }, [source, selectedSubtitle, subtitleRetry, subtitleWindowStart, savedSubtitles]);

  useEffect(() => {
    const media = mediaRef.current;
    if (!(media instanceof HTMLVideoElement)) return;
    const selectedTrack = subtitleTrackRef.current?.track;
    const useNativeCaptions = pictureInPicture;
    const updateCaption = () => {
      if (useNativeCaptions || selectedSubtitle === null || !subtitleUrl) { setCaptionText(""); return; }
      setCaptionText(visibleSubtitle(subtitleCuesRef.current, media.currentTime));
    };
    for (const track of Array.from(media.textTracks)) {
      track.mode = track === selectedTrack && subtitleUrl
        ? (useNativeCaptions ? "showing" : "hidden") : "disabled";
    }
    selectedTrack?.addEventListener("cuechange", updateCaption);
    media.addEventListener("seeking", updateCaption);
    media.addEventListener("seeked", updateCaption);
    media.addEventListener("timeupdate", updateCaption);
    updateCaption();
    return () => {
      selectedTrack?.removeEventListener("cuechange", updateCaption);
      media.removeEventListener("seeking", updateCaption);
      media.removeEventListener("seeked", updateCaption);
      media.removeEventListener("timeupdate", updateCaption);
    };
  }, [source, playbackUrl, selectedSubtitle, pictureInPicture, subtitleUrl, subtitleTrackVersion]);

  function revealControls() {
    window.clearTimeout(controlsTimer.current);
    setControlsVisible(true);
    if (playing && !subtitleMenuOpen && !audioMenuOpen) {
      controlsTimer.current = window.setTimeout(() => setControlsVisible(false), 3500);
    }
  }

  async function importSubtitle(file: File) {
    try {
      if (file.size > 2_000_000) throw new Error("Subtitle files must be under 2 MB.");
      const vtt = toWebVtt(await file.text(), file.name);
      const language = file.name.match(/\.([a-z]{2,3})\.(?:srt|vtt)$/i)?.[1]?.toLowerCase() ?? "und";
      const saved: SavedSubtitle = { id: crypto.randomUUID(), itemId: item.Id, name: file.name, language, vtt };
      await saveSubtitle(saved);
      setSavedSubtitles((current) => [...current, saved]);
      setSelectedSubtitle(saved.id);
      setSubtitleMenuOpen(false);
      setSubtitleError(null);
    } catch (reason) { setSubtitleError(reason instanceof Error ? reason.message : "Could not import subtitle file."); }
  }

  async function searchMoreSubtitles() {
    setSubtitleSearchLoading(true);
    setSubtitleSearchError(null);
    try {
      const result = await api.searchSubtitles(item.Id, subtitleLanguage.trim());
      setSubtitleResults(result.Results);
    } catch (reason) { setSubtitleSearchError(reason instanceof Error ? reason.message : "Subtitle search failed."); }
    finally { setSubtitleSearchLoading(false); }
  }

  async function downloadMoreSubtitle(result: SubtitleSearchResult) {
    setSubtitleSearchLoading(true);
    setSubtitleSearchError(null);
    try {
      const downloaded = await api.downloadSubtitle(item.Id, result.FileId);
      const vtt = toWebVtt(downloaded.Content, `subtitle.${downloaded.Format}`);
      const saved: SavedSubtitle = { id: crypto.randomUUID(), itemId: item.Id,
        name: result.FileName, language: result.Language, vtt };
      await saveSubtitle(saved);
      setSavedSubtitles((current) => [...current, saved]);
      setSelectedSubtitle(saved.id);
      setSubtitleSearchOpen(false);
      setSubtitleMenuOpen(false);
    } catch (reason) { setSubtitleSearchError(reason instanceof Error ? reason.message : "Subtitle download failed."); }
    finally { setSubtitleSearchLoading(false); }
  }

  function togglePlayback() {
    const media = mediaRef.current;
    if (!media) return;
    if (media.paused) {
      void media.play().catch((reason: unknown) => {
        if (media !== mediaRef.current || !media.paused) return;
        const name = reason && typeof reason === "object" && "name" in reason ? String(reason.name) : "";
        if (name === "AbortError") return;
        if (name === "NotSupportedError" && playbackUrl === source?.DirectStreamUrl && source.FallbackTranscodingUrl) {
          setPlaybackUrl(source.FallbackTranscodingUrl);
          return;
        }
        setPlaybackNotice(name === "NotAllowedError"
          ? "The browser blocked playback. Press Play to try again."
          : "The stream is still loading. Press Play to try again.");
      });
    } else media.pause();
    revealControls();
  }

  function seek(seconds: number) {
    const media = mediaRef.current;
    if (!media || !Number.isFinite(media.duration)) return;
    media.currentTime = Math.max(0, Math.min(seconds, media.duration));
    setCurrentTime(media.currentTime);
    positionRef.current = Math.floor(media.currentTime * TICKS_PER_SECOND);
    revealControls();
  }

  function selectAudioTrack(index: number) {
    if (!source || index === selectedAudio) { setAudioMenuOpen(false); return; }
    const url = source.AudioTrackUrls?.[String(index)];
    if (!url) { setPlaybackNotice("This soundtrack is unavailable."); return; }
    const position = mediaRef.current?.currentTime ?? currentTime;
    switchPositionRef.current = position;
    const address = new URL(url, window.location.href);
    address.searchParams.set("PlaySessionId", crypto.randomUUID().replace(/-/g, ""));
    address.searchParams.set("StartIndex", String(Math.floor(position / 6)));
    setSelectedAudio(index);
    setPlaybackUrl(address.pathname + address.search);
    setAudioMenuOpen(false);
    setPlaybackNotice(null);
  }

  async function toggleFullscreen() {
    if (document.fullscreenElement) await document.exitFullscreen();
    else if (frameRef.current?.requestFullscreen) await frameRef.current.requestFullscreen();
    else if (mediaRef.current instanceof HTMLVideoElement) {
      const video = mediaRef.current as HTMLVideoElement & { webkitEnterFullscreen?: () => void };
      video.webkitEnterFullscreen?.();
    }
  }

  async function togglePictureInPicture() {
    const video = mediaRef.current;
    if (!(video instanceof HTMLVideoElement)) return;
    const safariVideo = video as HTMLVideoElement & { webkitPresentationMode?: string; webkitSetPresentationMode?: (mode: string) => void };
    try {
      if (document.pictureInPictureElement === video) await document.exitPictureInPicture();
      else if (safariVideo.webkitPresentationMode === "picture-in-picture") safariVideo.webkitSetPresentationMode?.("inline");
      else if (document.pictureInPictureEnabled && video.requestPictureInPicture) await video.requestPictureInPicture();
      else safariVideo.webkitSetPresentationMode?.("picture-in-picture");
    } catch { /* Browser permission or device support can change at runtime. */ }
  }

  useEffect(() => {
    const media = mediaRef.current;
    if (!media || !playbackUrl?.includes(".m3u8")) return;
    const nativeHls = navigator.vendor.includes("Apple")
      && media.canPlayType("application/vnd.apple.mpegurl") !== "";
    if (nativeHls) return;
    let disposed = false;
    let player: import("hls.js").default | undefined;
    void import("hls.js").then(({ default: Hls }) => {
      if (disposed) return;
      if (!Hls.isSupported()) { setError("This browser cannot play the transcoded HLS stream."); return; }
      const startPosition = switchPositionRef.current ?? ticksToSeconds(item.UserData?.Played ? 0 : item.UserData?.PlaybackPositionTicks);
      player = new Hls({ startPosition, maxBufferLength: 18, maxMaxBufferLength: 30, backBufferLength: 12, fragLoadingMaxRetry: 6 });
      player.loadSource(resolveUrl(playbackUrl));
      player.attachMedia(media);
      player.on(Hls.Events.ERROR, (_event, data) => {
        if (data.fatal) {
          if (!stoppedRef.current) {
            stoppedRef.current = true;
            void api.reportStopped(item.Id, positionRef.current, playSessionRef.current).catch(() => {});
          }
          setError(`The transcoded stream failed (${data.details}).`);
        }
      });
    });
    return () => { disposed = true; player?.destroy(); };
  }, [playbackUrl, item.UserData?.PlaybackPositionTicks, item.UserData?.Played]);

  useEffect(() => {
    const stop = () => {
      if (stoppedRef.current) return;
      stoppedRef.current = true;
      void api.reportStopped(item.Id, positionRef.current, playSessionRef.current).catch(() => {});
    };
    window.addEventListener("pagehide", stop);
    const interval = window.setInterval(() => {
      const position = positionRef.current;
      if (!stoppedRef.current && position > 0 && Math.abs(position - lastReportedRef.current) >= TICKS_PER_SECOND * 10) {
        lastReportedRef.current = position;
        void api.reportProgress(item.Id, position, playSessionRef.current).catch(() => {});
      }
    }, 5000);
    return () => {
      window.clearInterval(interval);
      window.removeEventListener("pagehide", stop);
      stop();
    };
  }, [item.Id]);

  if (error) return <div className="rounded-xl border border-danger/40 bg-danger/10 px-4 py-4 text-center" role="alert">
    <p className="font-medium">Playback is unavailable for this item.</p><p className="muted mt-1 text-sm">{error}</p>
  </div>;
  if (!source || !playbackUrl) return <Spinner label="Preparing playback…" />;

  const subtitles = source.MediaStreams.filter((stream: MediaStream) =>
    stream.Type === "Subtitle" && stream.DeliveryUrl && stream.Index !== undefined);
  const unavailableSubtitles = source.MediaStreams.filter((stream: MediaStream) =>
    stream.Type === "Subtitle" && (!stream.DeliveryUrl || stream.Index === undefined));
  const audioTracks = source.MediaStreams.filter((stream: MediaStream) => stream.Type === "Audio" && stream.Index !== undefined);
  const isHls = playbackUrl.includes(".m3u8");
  const nativeHls = navigator.vendor.includes("Apple")
    && document.createElement("video").canPlayType("application/vnd.apple.mpegurl") !== "";
  const savedPosition = ticksToSeconds(item.UserData?.Played ? 0 : item.UserData?.PlaybackPositionTicks);
  const initialUrl = resolveUrl(playbackUrl);
  const startPosition = switchPositionRef.current ?? savedPosition;
  const nativeSrc = !isHls || nativeHls
    ? `${initialUrl}${startPosition > 0 ? `#t=${startPosition}` : ""}`
    : undefined;
  const audioConversion = playbackUrl === source.AudioTranscodingUrl;
  const supportsPiP = document.pictureInPictureEnabled || "webkitSetPresentationMode" in HTMLVideoElement.prototype;
  const updatePosition = () => {
    const media = mediaRef.current;
    if (!media) return;
    positionRef.current = Math.floor(media.currentTime * TICKS_PER_SECOND);
    setCurrentTime(media.currentTime);
    if (item.IsRemote && typeof selectedSubtitle === "number" && source.MediaStreams.find((stream) => stream.Index === selectedSubtitle)?.IsExternal !== true)
      setSubtitleWindowStart(Math.floor(media.currentTime / 45) * 45);
  };
  const commonProps = {
    src: nativeSrc, controls: isAudio, autoPlay: true, preload: "auto" as const,
    onPlay: () => { setPlaying(true); setPlaybackNotice(null); },
    onPause: () => { setPlaying(false); setControlsVisible(true); window.clearTimeout(controlsTimer.current); },
    onVolumeChange: () => { const media = mediaRef.current; if (media) { setVolume(media.volume); setMuted(media.muted); } },
    onTimeUpdate: updatePosition,
    onSeeking: updatePosition,
    onSeeked: updatePosition,
    onLoadedMetadata: () => {
      const media = mediaRef.current; if (!media) return; setDuration(media.duration);
      const saved = switchPositionRef.current ?? savedPosition;
      if (saved > 0 && (switchPositionRef.current !== null || !item.UserData?.Played) && media.currentTime === 0 && Number.isFinite(media.duration))
        media.currentTime = Math.max(0, Math.min(saved, media.duration - 1));
    },
    onEnded: () => {
      setPlaying(false);
      if (stoppedRef.current) return; stoppedRef.current = true;
      void api.reportStopped(item.Id, positionRef.current, playSessionRef.current).catch(() => {});
    },
    onError: () => {
      if (playbackUrl === source.DirectStreamUrl && source.FallbackTranscodingUrl) {
        setError(null); setPlaybackUrl(source.FallbackTranscodingUrl);
        setPlaybackNotice(null);
      } else {
        if (!stoppedRef.current) {
          stoppedRef.current = true;
          void api.reportStopped(item.Id, positionRef.current, playSessionRef.current).catch(() => {});
        }
        setError("The media element could not play the stream.");
      }
    },
  };

  const playerControlsShown = controlsVisible || !playing || subtitleMenuOpen || audioMenuOpen;
  return <div className={`mx-auto ${isAudio ? "max-w-2xl" : "max-w-4xl"}`}>
    {isAudio ? <audio {...commonProps} ref={mediaRef as RefObject<HTMLAudioElement>}
      className="w-full rounded-xl border border-edge bg-surface-raised p-3" />
      : <div ref={frameRef} className="group relative overflow-hidden rounded-xl border border-edge bg-black shadow-lg"
          onMouseMove={revealControls} onMouseLeave={() => { if (playing && !subtitleMenuOpen && !audioMenuOpen) setControlsVisible(false); }}
          onFocusCapture={revealControls} onContextMenu={(event) => event.preventDefault()}>
          <video {...commonProps} ref={mediaRef as RefObject<HTMLVideoElement>}
            className="aspect-video w-full cursor-pointer bg-black object-contain" playsInline
            onClick={togglePlayback}>
            {subtitleUrl && <track ref={subtitleTrackRef} key={selectedSubtitle} kind="subtitles" src={subtitleUrl}
              srcLang={typeof selectedSubtitle === "string"
                ? savedSubtitles.find((saved) => saved.id === selectedSubtitle)?.language ?? "und"
                : subtitles.find((stream) => stream.Index === selectedSubtitle)?.Language ?? "und"}
              label="Selected subtitles" onLoad={() => setSubtitleTrackVersion((version) => version + 1)}
              onError={() => setSubtitleError("The browser could not read this subtitle track. Choose it to retry.")} />}
          </video>
          {playbackNotice && <div className="pointer-events-none absolute inset-x-4 top-4 z-20 flex justify-center" role="status">
            <span className="rounded-md bg-black/80 px-3 py-2 text-sm text-white">{playbackNotice}</span>
          </div>}
          {subtitleError && <div className="pointer-events-none absolute inset-x-4 top-4 z-20 flex justify-center" role="status">
            <span className="rounded-md bg-black/80 px-3 py-2 text-sm text-amber-200">{subtitleError}</span>
          </div>}
          {captionText && <div className={`pointer-events-none absolute inset-x-5 z-10 flex justify-center text-center transition-[bottom] duration-200 ${playerControlsShown ? "bottom-24 sm:bottom-20" : "bottom-5 sm:bottom-7"}`} aria-live="off">
            <span className="max-w-[90%] whitespace-pre-line rounded bg-black/75 px-3 py-1.5 text-base font-semibold leading-snug text-white shadow-lg sm:text-lg">{captionText}</span>
          </div>}
          <div className={`absolute inset-x-0 bottom-0 bg-gradient-to-t from-black/95 via-black/75 to-transparent px-3 pb-3 pt-10 text-white transition-opacity duration-200 sm:px-5 ${playerControlsShown ? "opacity-100" : "pointer-events-none opacity-0"}`}>
            <input type="range" min={0} max={duration || 0} step={0.1} value={Math.min(currentTime, duration || 0)}
              onChange={(event) => seek(Number(event.target.value))} aria-label="Seek playback"
              className="w-full cursor-pointer accent-brand" />
            <div className="mt-1 flex flex-wrap items-center gap-1.5 text-sm sm:gap-3">
              <button type="button" onClick={togglePlayback} aria-label={playing ? "Pause" : "Play"}
                className="rounded-full p-2 hover:bg-white/15 focus-visible:outline-2 focus-visible:outline-brand"><PlaybackIcon name={playing ? "pause" : "play"} /></button>
              <button type="button" onClick={() => seek(currentTime - 10)} aria-label="Back 10 seconds"
                className="rounded-full p-2 hover:bg-white/15 focus-visible:outline-2 focus-visible:outline-brand"><PlaybackIcon name="back10" className="h-6 w-6" /></button>
              <button type="button" onClick={() => seek(currentTime + 10)} aria-label="Forward 10 seconds"
                className="rounded-full p-2 hover:bg-white/15 focus-visible:outline-2 focus-visible:outline-brand"><PlaybackIcon name="forward10" className="h-6 w-6" /></button>
              <span className="min-w-24 tabular-nums text-xs text-white/85">{formatSeconds(currentTime)} / {formatSeconds(duration)}</span>
              <div className="ml-auto flex items-center gap-1.5 sm:gap-3">
                <button type="button" aria-label={muted || volume === 0 ? "Unmute" : "Mute"}
                  onClick={() => { if (mediaRef.current) mediaRef.current.muted = !mediaRef.current.muted; }}
                  className="rounded-full p-2 hover:bg-white/15 focus-visible:outline-2 focus-visible:outline-brand"><PlaybackIcon name={muted || volume === 0 ? "muted" : "volume"} /></button>
                <input type="range" min={0} max={1} step={0.05} value={volume} aria-label="Volume"
                  onChange={(event) => { if (mediaRef.current) { mediaRef.current.volume = Number(event.target.value); mediaRef.current.muted = false; } }}
                  className="hidden w-20 cursor-pointer accent-brand sm:block" />
                {audioTracks.length > 1 && <div className="relative">
                  <button type="button" aria-label="Soundtracks" aria-expanded={audioMenuOpen} aria-haspopup="menu"
                    onClick={() => { setAudioMenuOpen((open) => !open); setSubtitleMenuOpen(false); setControlsVisible(true); }}
                    className="rounded-full p-2 hover:bg-white/15 focus-visible:outline-2 focus-visible:outline-brand"><PlaybackIcon name="audioTracks" /></button>
                  {audioMenuOpen && <div role="menu" aria-label="Audio tracks" className="absolute bottom-full right-0 z-20 mb-2 max-h-72 min-w-52 overflow-y-auto rounded-lg border border-white/15 bg-surface-raised p-1 shadow-xl">
                    {audioTracks.map((track, ordinal) => <button key={track.Index} type="button" role="menuitemradio"
                      aria-checked={selectedAudio === track.Index} onClick={() => selectAudioTrack(track.Index!)}
                      className="block w-full rounded px-3 py-2 text-left text-sm hover:bg-white/10">
                      {track.Language && track.Language.toLowerCase() !== "und" ? track.Language.toUpperCase() : `Track ${ordinal + 1}`}
                      {track.Title ? ` · ${track.Title}` : ""}{track.IsDefault ? " · Default" : ""}{track.Codec ? ` · ${track.Codec.toUpperCase()}` : ""}
                    </button>)}
                  </div>}
                </div>}
                <div className="relative">
                  <input ref={subtitleFileInputRef} type="file" accept=".srt,.vtt,text/plain,text/vtt" className="hidden"
                    aria-label="Import subtitle file" onChange={(event) => {
                      const file = event.currentTarget.files?.[0];
                      if (file) void importSubtitle(file);
                      event.currentTarget.value = "";
                    }} />
                  <button type="button" aria-label="Subtitles" aria-expanded={subtitleMenuOpen} aria-haspopup="menu"
                    onClick={() => { setSubtitleMenuOpen(!subtitleMenuOpen); setAudioMenuOpen(false); setControlsVisible(true); }}
                    className={`rounded-full p-2 hover:bg-white/15 focus-visible:outline-2 focus-visible:outline-brand ${selectedSubtitle !== null ? "text-brand-strong" : ""}`}><PlaybackIcon name="captions" /></button>
                  {subtitleMenuOpen && <div role="menu" aria-label="Subtitle tracks" className="absolute bottom-full right-0 z-20 mb-2 max-h-80 min-w-56 overflow-y-auto rounded-lg border border-white/15 bg-surface-raised p-1 shadow-xl">
                    {subtitleLoading && <p className="px-3 py-1 text-xs text-white/65">Loading subtitles…</p>}
                    {subtitleError && <p className="px-3 py-1 text-xs text-amber-300" role="status">{subtitleError}</p>}
                    <button type="button" role="menuitemradio" aria-checked={selectedSubtitle === null}
                      onClick={() => { setSelectedSubtitle(null); setSubtitleMenuOpen(false); }}
                      className="block w-full rounded px-3 py-2 text-left text-sm hover:bg-white/10">Off</button>
                    {subtitles.length === 0 && savedSubtitles.length === 0 && <p className="px-3 py-2 text-xs text-white/70">
                      {unavailableSubtitles.length > 0
                        ? "This item has subtitles, but none are available as browser-compatible text."
                        : "No subtitles are available for this item."}
                    </p>}
                    {subtitles.map((stream) => <button key={stream.Index} type="button" role="menuitemradio"
                      aria-checked={selectedSubtitle === stream.Index}
                      onClick={() => { setSubtitleWindowStart(Math.floor((mediaRef.current?.currentTime ?? 0) / 45) * 45); setSelectedSubtitle(stream.Index ?? null); setSubtitleRetry((count) => count + 1); setSubtitleMenuOpen(false); }}
                      className="block w-full rounded px-3 py-2 text-left text-sm hover:bg-white/10">
                      {stream.Language && stream.Language.toLowerCase() !== "und" ? stream.Language.toUpperCase() : "Unknown language"}{stream.Title ? ` · ${stream.Title}` : ""}{stream.IsDefault ? " · Default" : ""}{stream.IsForced ? " · Forced only" : ""}
                      {" · "}{stream.IsExternal ? "external" : "embedded"}{" "}{stream.Codec?.toUpperCase() ?? "text"}
                      {stream.Language && stream.Language.toLowerCase() !== "und" ? "" : ` · #${stream.Index}`}
                    </button>)}
                    {savedSubtitles.map((saved) => <div key={saved.id} className="flex items-center gap-1">
                      <button type="button" role="menuitemradio" aria-checked={selectedSubtitle === saved.id}
                        onClick={() => { setSelectedSubtitle(saved.id); setSubtitleMenuOpen(false); }}
                        className="min-w-0 flex-1 truncate rounded px-3 py-2 text-left text-sm hover:bg-white/10"
                        title={saved.name}>{saved.language.toUpperCase()} · {saved.name}</button>
                      <button type="button" aria-label={`Remove ${saved.name}`} className="rounded px-2 py-1 text-xs text-white/60 hover:bg-white/10"
                        onClick={() => { void deleteSubtitle(saved.id).then(() => {
                          setSavedSubtitles((current) => current.filter((entry) => entry.id !== saved.id));
                          if (selectedSubtitle === saved.id) setSelectedSubtitle(null);
                        }).catch(() => setSubtitleError("Could not remove subtitle.")); }}>×</button>
                    </div>)}
                    <div className="my-1 border-t border-white/15" />
                    <button type="button" role="menuitem" onClick={() => subtitleFileInputRef.current?.click()}
                      className="block w-full rounded px-3 py-2 text-left text-sm hover:bg-white/10">Import SRT/VTT file…</button>
                    <button type="button" role="menuitem" onClick={() => setSubtitleSearchOpen((open) => !open)}
                      className="block w-full rounded px-3 py-2 text-left text-sm hover:bg-white/10">Get more from OpenSubtitles…</button>
                    {subtitleSearchOpen && <div className="space-y-2 border-t border-white/15 px-2 py-2">
                      <div className="flex gap-1">
                        <input value={subtitleLanguage} onChange={(event) => setSubtitleLanguage(event.target.value)}
                          aria-label="Subtitle language" maxLength={3} className="w-12 rounded bg-black/40 px-1 text-sm" />
                        <button type="button" onClick={() => void searchMoreSubtitles()} disabled={subtitleSearchLoading}
                          className="rounded bg-brand/30 px-2 py-1 text-xs hover:bg-brand/50">Search</button>
                      </div>
                      {subtitleSearchError && <p role="status" className="text-xs text-amber-300">{subtitleSearchError}</p>}
                      {subtitleSearchLoading && <p className="text-xs text-white/65">Searching subtitles…</p>}
                      {!subtitleSearchLoading && subtitleResults.length === 0 && !subtitleSearchError && <p className="text-xs text-white/65">Search by language to find subtitles.</p>}
                      {subtitleResults.map((result) => <button key={result.FileId} type="button" role="menuitem"
                        onClick={() => void downloadMoreSubtitle(result)} disabled={subtitleSearchLoading}
                        className="block w-full rounded px-1 py-2 text-left text-xs hover:bg-white/10">
                        <span className="block truncate" title={result.FileName}>{result.FileName}</span>
                        <span className="text-white/55">{result.Language.toUpperCase()} · {result.DownloadCount} downloads{result.HearingImpaired ? " · SDH" : ""}</span>
                      </button>)}
                    </div>}
                  </div>}
                </div>
                {supportsPiP && <button type="button" onClick={() => void togglePictureInPicture()} aria-label={pictureInPicture ? "Exit picture-in-picture" : "Picture-in-picture"}
                  className="rounded-full p-2 hover:bg-white/15 focus-visible:outline-2 focus-visible:outline-brand"><PlaybackIcon name={pictureInPicture ? "exitPip" : "pip"} /></button>}
                <button type="button" onClick={() => void toggleFullscreen()} aria-label={fullscreen ? "Exit fullscreen" : "Fullscreen"}
                  className="rounded-full p-2 hover:bg-white/15 focus-visible:outline-2 focus-visible:outline-brand"><PlaybackIcon name={fullscreen ? "exitFullscreen" : "fullscreen"} /></button>
              </div>
            </div>
          </div>
        </div>}
    <div className="mt-3 flex flex-wrap items-baseline justify-between gap-2">
      <span className="font-medium">{item.Name}</span><span className="muted text-sm">
        {formatSeconds(currentTime)}{duration > 0 && ` / ${formatSeconds(duration)}`}
        {source.Container ? ` · ${source.Container.toUpperCase()}` : ""}
        {audioConversion && source.AudioTranscodingMode === "audio" && " · Converting audio"}
        {audioConversion && source.AudioTranscodingMode === "video" && " · Transcoding for browser compatibility"}
      </span>
    </div>
  </div>;
}

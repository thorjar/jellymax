import type { MediaStream } from "../api/types";

function playable(element: HTMLMediaElement, mime: string): boolean {
  return element.canPlayType(mime).replace(/no/, "") !== "";
}

// The Safari-relevant profile rules used by jellyfin-web: capability checks
// come from canPlayType, MKV is never assumed, and HLS uses fMP4.
export function buildBrowserDeviceProfile(supportsHevc = browserSupportsHevc()) {
  const video = document.createElement("video");
  const audio = document.createElement("audio");
  const appleNativeHls = navigator.vendor.includes("Apple")
    && playable(video, "application/vnd.apple.mpegurl");
  const h264 = playable(video, 'video/mp4; codecs="avc1.42E01E, mp4a.40.2"');
  const hevc = supportsHevc;
  const aac = playable(video, 'video/mp4; codecs="avc1.640029, mp4a.40.2"');
  const ac3 = playable(video, 'audio/mp4; codecs="ac-3"');
  const eac3 = playable(video, 'audio/mp4; codecs="ec-3"');
  const opus = playable(audio, 'audio/ogg; codecs="opus"');
  const flac = playable(audio, "audio/flac");
  const canPlayMkv = browserSupportsMkv();
  const videoCodecs=[h264&&"h264",hevc&&"hevc"].filter(Boolean) as string[];
  const directAudio=[aac&&"aac",ac3&&"ac3",eac3&&"eac3",opus&&"opus",flac&&"flac"].filter(Boolean) as string[];
  const hlsAudio=[aac&&"aac"].filter(Boolean) as string[];
  const directPlayProfiles:Record<string,string>[]=[];
  if(videoCodecs.length)directPlayProfiles.push({Container:"mp4,m4v",Type:"Video",VideoCodec:videoCodecs.join(","),AudioCodec:directAudio.join(",")});
  if(canPlayMkv&&videoCodecs.length)directPlayProfiles.push({Container:"mkv",Type:"Video",VideoCodec:videoCodecs.join(","),AudioCodec:directAudio.join(",")});
  directPlayProfiles.push({Container:"hls",Type:"Video",VideoCodec:videoCodecs.join(","),AudioCodec:hlsAudio.join(",")});
  return {Name:"Jellymax Web",MaxStreamingBitrate:10_000_000,MaxStaticBitrate:120_000_000,MusicStreamingTranscodingBitrate:384_000,
    DirectPlayProfiles:directPlayProfiles,TranscodingProfiles:[{Container:"mp4",Type:"Video",VideoCodec:videoCodecs.join(",")||"h264",AudioCodec:hlsAudio.join(",")||"aac",Context:"Streaming",Protocol:"hls",MaxAudioChannels:"6",MinSegments:appleNativeHls?"1":"2",SegmentLength:appleNativeHls?2:undefined,BreakOnNonKeyFrames:true}],ContainerProfiles:[],CodecProfiles:supportsHevc?[]:[{Type:"Video",Conditions:[{Condition:"LessThanEqual",Property:"Width",Value:"1280",IsRequired:false}]}],SubtitleProfiles:["vtt","srt","sub","ass","ssa"].map(Format=>({Format,Method:"External"}))};
}

export function browserSupportsHevc():boolean {
  // Chrome on Apple platforms can under-report HEVC through canPlayType even
  // though the platform decoder is available. Safari uses that decoder too.
  if (/Macintosh|Mac OS X|iPhone|iPad|iPod/.test(navigator.userAgent)) return true;
  const video=document.createElement("video");
  return ['video/mp4; codecs="hvc1.1.L120"','video/mp4; codecs="hev1.1.L120"'].some(type=>playable(video,type));
}

export function browserSupportsMkv():boolean {
  const video = document.createElement("video");
  // Chromium may play Matroska even when canPlayType does not advertise it.
  // The player falls back to HLS if the direct source fails.
  return !navigator.vendor.includes("Apple")
    || playable(video, "video/x-matroska")
    || playable(video, "video/mkv");
}

export function safariCanDirectPlay(container: string | undefined, streams: MediaStream[]): boolean {
  if (!navigator.vendor.includes("Apple")) return false;
  const video = streams.find((stream) => stream.Type === "Video")?.Codec?.toLowerCase();
  const audio = streams.find((stream) => stream.Type === "Audio")?.Codec?.toLowerCase();
  const formats: Record<string, { mime: string; video: Record<string, string>; audio: Record<string, string> }> = {
    mp4: { mime: "video/mp4", video: { h264: "avc1.42E01E", hevc: "hvc1.1.L120" },
      audio: { aac: "mp4a.40.2", mp3: "mp4a.69", ac3: "ac-3", eac3: "ec-3" } },
    webm: { mime: "video/webm", video: { vp8: "vp8", vp9: "vp9", av1: "av01.0.05M.08" },
      audio: { opus: "opus", vorbis: "vorbis" } },
    mkv: { mime: "video/x-matroska", video: { h264: "avc1.42E01E", hevc: "hvc1.1.L120", vp9: "vp9" },
      audio: { aac: "mp4a.40.2", ac3: "ac-3", eac3: "ec-3", opus: "opus" } },
  };
  const format = formats[container?.toLowerCase() ?? ""];
  if (!format || !video || !format.video[video] || (audio && !format.audio[audio])) return false;
  const codecs = [format.video[video], audio && format.audio[audio]].filter(Boolean).join(", ");
  return playable(document.createElement("video"), `${format.mime}; codecs="${codecs}"`);
}

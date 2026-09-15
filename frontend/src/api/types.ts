// TypeScript mirrors of the JSON returned by the jellymax backend.
// The backend serializes with PascalCase field names.

export interface Policy {
  IsAdministrator: boolean;
}

export interface User {
  Id: string;
  Name: string;
  Policy: Policy;
}

export interface LoginResponse {
  User: User;
  AccessToken: string;
  ServerId: string;
}

export interface SystemInfo {
  ServerName: string;
  Id: string;
  Version: string;
  ProductName: string;
  StartupWizardCompleted: boolean;
}

export interface Library {
  ItemId: string;
  Name: string;
  CollectionType: string;
  Locations: string[];
  IsRemote: boolean;
  RemoteServerName?: string | null;
}

export interface MediaStream {
  Index?: number;
  Type?: string;
  Codec?: string;
  Width?: number;
  Height?: number;
  Channels?: number;
  SampleRate?: number;
  Language?: string;
  Title?: string;
  DisplayTitle?: string;
  IsDefault?: boolean;
  IsForced?: boolean;
  IsExternal?: boolean;
  DeliveryMethod?: string;
  DeliveryUrl?: string;
}

export interface SubtitleSearchResult {
  FileId: number;
  FileName: string;
  Language: string;
  DownloadCount: number;
  HearingImpaired: boolean;
}

export interface UserData {
  PlaybackPositionTicks: number;
  Played: boolean;
  IsFavorite: boolean;
}

export interface Item {
  Id: string;
  IsRemote: boolean;
  Name: string;
  Type: string;
  MediaType: "Audio" | "Video";
  IsFolder: boolean;
  ParentId?: string;
  LibraryId?: string;
  IndexNumber?: number | null;
  ParentIndexNumber?: number | null;
  ChildCount?: number;
  SeriesId?: string | null;
  SeriesName?: string | null;
  SeriesTmdbId?: string | null;
  Container?: string;
  Size?: number;
  RunTimeTicks?: number | null;
  MediaStreams?: MediaStream[];
  UserData?: UserData;
  /** TMDb identifier; set when metadata enrichment has run. */
  TmdbId?: string | null;
  /** Release year from TMDb or the file name. */
  Year?: number | null;
  Overview?: string | null;
  Genres?: string[];
  CommunityRating?: number | null;
}

export interface MediaSource {
  Id: string;
  Protocol: string;
  Type: string;
  Container: string;
  Size: number;
  RunTimeTicks?: number | null;
  MediaStreams: MediaStream[];
  SupportsDirectPlay: boolean;
  SupportsDirectStream: boolean;
  SupportsTranscoding: boolean;
  DirectStreamUrl: string;
  TranscodingUrl?: string | null;
  FallbackTranscodingUrl?: string | null;
  AudioTranscodingUrl?: string | null;
  AudioTrackUrls?: Record<string, string>;
  AudioTranscodingMode?: "audio" | "audio-only" | "video";
  TranscodingMode?: "remux" | "audio" | "video" | null;
}

export interface PlaybackInfo {
  PlaySessionId: string;
  MediaSources: MediaSource[];
}

export interface Playlist {
  Id: string;
  Name: string;
  Type: string;
  ChildCount: number;
}

export interface ScanStatus {
  Id: string;
  State: string;
  Scanned: number;
  Removed: number;
  ProbeFailures: number;
  MetadataFailures: number;
  /** "TMDb" when a TMDb API key is configured, otherwise "Disabled". */
  MetadataProvider: string;
  Errors: string[];
}

export interface ItemList {
  Items: Item[];
  TotalRecordCount: number;
  StartIndex: number;
}

export interface Recommendation {
  BaselineItemName?: string | null;
  CategoryId: string;
  RecommendationType: string;
  Items: Item[];
}

export interface PlaylistList {
  Items: Playlist[];
  TotalRecordCount: number;
}

export interface DirectoryChild {
  Name: string;
  Path: string;
}

export interface DirectoryListing {
  Path?: string;
  Parent?: string | null;
  Directories: DirectoryChild[];
}

export interface RemoteServer {
  Id: string;
  Name: string;
  Url: string;
  ServerId: string;
  LastSync?: number | null;
  LastError?: string | null;
}

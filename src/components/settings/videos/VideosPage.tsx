import React, { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { AlertTriangle, FileText, FolderOpen, Loader2, Trash2, Video } from "lucide-react";
import { commands, events, type VideoMeta } from "@/bindings";
import { formatShortDateTime } from "@/utils/dateFormat";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { Button } from "../../ui/Button";
import { Dropdown } from "../../ui/Dropdown";

/** "4:05" or "1:02:03". */
function duration(s: number): string {
  const total = Math.max(0, Math.round(s));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const ss = String(total % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${ss}` : `${m}:${ss}`;
}

/**
 * Videos: screen and camera recordings. "New video" opens the recording bar;
 * each video can change its look (rendered again), open its transcript and
 * show its files in Finder.
 */
export const VideosPage: React.FC = () => {
  const { t, i18n } = useTranslation();
  const [videos, setVideos] = useState<VideoMeta[]>([]);

  const reload = useCallback(async () => {
    const r = await commands.videoList();
    if (r.status === "ok") setVideos(r.data);
  }, []);

  useEffect(() => {
    void reload();
    const unlisten = [
      events.videoChanged.listen(() => void reload()),
      events.meetingChanged.listen(() => void reload()),
    ];
    return () => unlisten.forEach((u) => u.then((fn) => fn()));
  }, [reload]);

  const looks = [
    { value: "bubble", label: t("video.bar.lookBubble") },
    { value: "side", label: t("video.bar.lookSide") },
    { value: "vertical", label: t("video.bar.lookVertical") },
    { value: "files", label: t("video.bar.lookFiles") },
  ];

  const reveal = (v: VideoMeta) =>
    commands.videoReveal(v.rendered ? `${v.folder}/${v.rendered}` : v.folder);

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <div className="px-1 flex items-start justify-between gap-4">
        <div className="min-w-0">
          <h1 className="font-display text-title font-[650] leading-[1.15] tracking-[-0.015em]">
            {t("video.page.title")}
          </h1>
          <p className="mt-1 max-w-[60ch] text-body text-ink-2">{t("video.page.description")}</p>
        </div>
        <Button variant="primary" size="md" onClick={() => void commands.videoBarShow()}>
          {t("video.page.new")}
        </Button>
      </div>

      {videos.length === 0 ? (
        <div className="mx-1 flex h-64 flex-col items-center justify-center gap-3 rounded-2xl border border-dashed border-line-strong text-ink-3">
          <Video width={28} height={28} strokeWidth={1.5} />
          {t("video.page.empty")}
        </div>
      ) : (
        <SettingsGroup>
          {videos.map((v) => (
            <div key={v.folder} className="group flex items-center justify-between gap-3 px-4 py-3">
              <div className="min-w-0">
                <div className="text-sm font-medium truncate">
                  {formatShortDateTime(String(Date.parse(v.started_at) / 1000), i18n.language)}
                </div>
                <div className="flex items-center gap-1.5 text-xs text-ink-3">
                  {v.status === "recording" && t("video.page.recording")}
                  {v.status === "rendering" && (
                    <>
                      <Loader2 width={12} height={12} className="animate-spin" />
                      {t("video.page.rendering")}
                    </>
                  )}
                  {v.status === "failed" && (
                    <span className="flex items-center gap-1 text-amber-600" title={v.error ?? undefined}>
                      <AlertTriangle width={12} height={12} />
                      {t("video.page.failed")}
                    </span>
                  )}
                  {v.status === "ready" && duration(v.duration_s)}
                </div>
              </div>
              <div className="flex shrink-0 items-center gap-1">
                {v.screen && v.camera && (
                  <Dropdown
                    options={looks}
                    selectedValue={v.look}
                    onSelect={(look) => void commands.videoSetLook(v.folder, look).then(reload)}
                    disabled={v.status === "recording" || v.status === "rendering"}
                    tone="neutral"
                  />
                )}
                <Button
                  variant="ghost"
                  size="sm"
                  title={t("video.page.transcript")}
                  onClick={() => void commands.openMeetingNote(v.meeting_id)}
                  disabled={v.status === "recording"}
                >
                  <FileText width={16} height={16} />
                </Button>
                <Button variant="ghost" size="sm" title={t("video.page.show")} onClick={() => void reveal(v)}>
                  <FolderOpen width={16} height={16} />
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  title={t("video.page.delete")}
                  onClick={() => void commands.videoDelete(v.folder).then(reload)}
                  disabled={v.status === "recording" || v.status === "rendering"}
                >
                  <Trash2 width={16} height={16} />
                </Button>
              </div>
            </div>
          ))}
        </SettingsGroup>
      )}
    </div>
  );
};

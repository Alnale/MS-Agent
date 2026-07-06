import { useState, useCallback } from 'react';
import type { MediaItemMeta } from '../hooks/useMediaLibrary';
import type { LgConfig } from '../App';
import { InterfaceSettings } from './settings/InterfaceSettings';
import { BubbleColorSettings } from './settings/BubbleColorSettings';
import { LiquidGlassSettings } from './settings/LiquidGlassSettings';
import { CompanionSettings } from './settings/CompanionSettings';
import { BackgroundSettings } from './settings/BackgroundSettings';

interface MediaLibraryData {
  images: MediaItemMeta[];
  videos: MediaItemMeta[];
  importFiles: (files: FileList | File[], folder?: string) => Promise<number>;
  importFilesByType: (type: 'image' | 'video' | 'music', files: FileList | File[], folder?: string) => Promise<number>;
  importFolder: (files: FileList, folder?: string) => Promise<number>;
  remove: (id: string) => Promise<void>;
  removeFolder?: (folder: string, type: 'image' | 'video' | 'music') => Promise<void>;
  removeAll?: (type: 'image' | 'video' | 'music') => Promise<void>;
  getUrl: (id: string) => Promise<string | null>;
}

interface Props {
  bgImage: string | null;
  bgVideo: string | null;
  bgOpacity: number;
  bgBlur: number;
  hideGlass: boolean;
  hideWelcomePrompt: boolean;
  useSolidBubble: boolean;
  bubbleTextColor: string;
  userBubbleColor: string;
  userBubbleAlpha: number;
  assistantBubbleColor: string;
  assistantBubbleAlpha: number;
  solidUserBubbleColor: string;
  solidAssistantBubbleColor: string;
  autoTextEnabled: boolean;
  onImageChange: (image: string | null) => void;
  onVideoChange: (video: string | null, file?: File) => void;
  onOpacityChange: (opacity: number) => void;
  onBlurChange: (blur: number) => void;
  onHideGlassChange: (hide: boolean) => void;
  onHideWelcomePromptChange: (hide: boolean) => void;
  onUseSolidBubbleChange: (use: boolean) => void;
  onBubbleTextColorChange: (color: string) => void;
  onUserBubbleColorChange: (color: string) => void;
  onUserBubbleAlphaChange: (alpha: number) => void;
  onAssistantBubbleColorChange: (color: string) => void;
  onAssistantBubbleAlphaChange: (alpha: number) => void;
  onSolidUserBubbleColorChange: (color: string) => void;
  onSolidAssistantBubbleColorChange: (color: string) => void;
  onAutoTextEnabledChange: (enabled: boolean) => void;
  companionMode: boolean;
  onCompanionModeChange: (enabled: boolean) => void;
  showEmotionPanel: boolean;
  onShowEmotionPanelChange: (show: boolean) => void;
  onClose: () => void;
  mediaLibrary?: MediaLibraryData;
  activeBgType?: 'image' | 'video' | null;
  lgConfig: LgConfig;
  onLgConfigChange: (config: LgConfig) => void;
}

export function SettingsPanel({
  bgImage, bgVideo, bgOpacity, bgBlur,
  hideGlass, hideWelcomePrompt, useSolidBubble,
  bubbleTextColor, userBubbleColor, userBubbleAlpha,
  assistantBubbleColor, assistantBubbleAlpha,
  solidUserBubbleColor, solidAssistantBubbleColor,
  autoTextEnabled,
  onImageChange, onVideoChange, onOpacityChange, onBlurChange,
  onHideGlassChange, onHideWelcomePromptChange,
  onUseSolidBubbleChange, onBubbleTextColorChange,
  onUserBubbleColorChange, onUserBubbleAlphaChange,
  onAssistantBubbleColorChange, onAssistantBubbleAlphaChange,
  onSolidUserBubbleColorChange, onSolidAssistantBubbleColorChange,
  onAutoTextEnabledChange,
  companionMode, onCompanionModeChange,
  showEmotionPanel, onShowEmotionPanelChange,
  onClose, mediaLibrary, activeBgType,
  lgConfig, onLgConfigChange,
}: Props) {
  const [closing, setClosing] = useState(false);

  const handleClose = useCallback(() => { setClosing(true); }, []);
  const handleAnimationEnd = useCallback(() => { if (closing) onClose(); }, [closing, onClose]);

  const bgLibData = mediaLibrary ? {
    images: mediaLibrary.images,
    videos: mediaLibrary.videos,
    importFilesByType: mediaLibrary.importFilesByType,
    importFolder: mediaLibrary.importFolder,
    remove: mediaLibrary.remove,
    removeFolder: mediaLibrary.removeFolder as MediaLibraryData['removeFolder'],
    removeAll: mediaLibrary.removeAll as MediaLibraryData['removeAll'],
    getUrl: mediaLibrary.getUrl,
  } : undefined;

  return (
    <div className={`settings-overlay${closing ? ' closing' : ''}`} onClick={handleClose} onAnimationEnd={handleAnimationEnd}>
      <div className={`settings-panel${closing ? ' closing' : ''}`} onClick={(e) => e.stopPropagation()}>
        <div className="settings-header">
          <h3>设置</h3>
          <button className="settings-close" onClick={handleClose}>
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
            </svg>
          </button>
        </div>

        <div className="settings-body">
          {/* Left column */}
          <div className="settings-col">
            <div className="settings-section">
              <InterfaceSettings
                hideGlass={hideGlass} hideWelcomePrompt={hideWelcomePrompt}
                useSolidBubble={useSolidBubble} autoTextEnabled={autoTextEnabled}
                bgVideo={bgVideo}
                onHideGlassChange={onHideGlassChange} onHideWelcomePromptChange={onHideWelcomePromptChange}
                onUseSolidBubbleChange={onUseSolidBubbleChange} onAutoTextEnabledChange={onAutoTextEnabledChange}
              />
              <div className="settings-divider" />
              <BubbleColorSettings
                useSolidBubble={useSolidBubble} bubbleTextColor={bubbleTextColor}
                userBubbleColor={userBubbleColor} userBubbleAlpha={userBubbleAlpha}
                assistantBubbleColor={assistantBubbleColor} assistantBubbleAlpha={assistantBubbleAlpha}
                solidUserBubbleColor={solidUserBubbleColor} solidAssistantBubbleColor={solidAssistantBubbleColor}
                autoTextEnabled={autoTextEnabled} bgVideo={bgVideo}
                onBubbleTextColorChange={onBubbleTextColorChange}
                onUserBubbleColorChange={onUserBubbleColorChange} onUserBubbleAlphaChange={onUserBubbleAlphaChange}
                onAssistantBubbleColorChange={onAssistantBubbleColorChange} onAssistantBubbleAlphaChange={onAssistantBubbleAlphaChange}
                onSolidUserBubbleColorChange={onSolidUserBubbleColorChange} onSolidAssistantBubbleColorChange={onSolidAssistantBubbleColorChange}
              />
              <div className="settings-divider" />
              <LiquidGlassSettings lgConfig={lgConfig} onLgConfigChange={onLgConfigChange} />
            </div>
            <CompanionSettings
              companionMode={companionMode} showEmotionPanel={showEmotionPanel}
              onCompanionModeChange={onCompanionModeChange} onShowEmotionPanelChange={onShowEmotionPanelChange}
            />
          </div>

          {/* Right column */}
          <div className="settings-col">
            <BackgroundSettings
              bgImage={bgImage} bgVideo={bgVideo} bgOpacity={bgOpacity} bgBlur={bgBlur}
              onImageChange={onImageChange} onVideoChange={onVideoChange}
              onOpacityChange={onOpacityChange} onBlurChange={onBlurChange}
              activeBgType={activeBgType} mediaLibrary={bgLibData}
            />
          </div>
        </div>
      </div>
    </div>
  );
}

export default SettingsPanel;

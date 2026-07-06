import { useEffect, useState, memo } from 'react';
import type { LgCategory } from '../App';
import { ShaderDisplacementGenerator, fragmentShaders } from '@liquid-glass/shader-utils';
import { displacementMap, polarDisplacementMap, prominentDisplacementMap } from '@liquid-glass/utils';

const FILTER_ID = 'lg-mask-overlay-filter';

const OVERLAY_STYLE: React.CSSProperties = {
  position: 'absolute',
  inset: 0,
  pointerEvents: 'none',
  zIndex: 1,
  overflow: 'hidden',
};

const SVG_STYLE: React.CSSProperties = {
  position: 'absolute',
  inset: 0,
  width: '100%',
  height: '100%',
  pointerEvents: 'none',
};

function getMap(mode: string, shaderMapUrl?: string): string {
  if (mode === "shader" && shaderMapUrl) return shaderMapUrl;
  if (mode === "polar") return polarDisplacementMap;
  if (mode === "prominent") return prominentDisplacementMap;
  return displacementMap;
}

function generateShaderDisplacementMap(width: number, height: number): string {
  const generator = new ShaderDisplacementGenerator({
    width,
    height,
    fragment: fragmentShaders.liquidGlass,
  });
  const dataUrl = generator.updateShader();
  generator.destroy();
  return dataUrl;
}

interface LgMaskOverlayProps {
  config: LgCategory;
  className?: string;
}

/**
 * Liquid Glass overlay for mask layers (status bar, input bar)
 * Uses SVG filter with edge-only displacement and chromatic aberration
 */
export const LgMaskOverlay = memo(function LgMaskOverlay({ config, className = '' }: LgMaskOverlayProps) {
  const [shaderMapUrl, setShaderMapUrl] = useState('');
  const { enabled, mode, displacementScale, blurAmount, saturation, aberrationIntensity, overLight } = config;

  useEffect(() => {
    if (mode === 'shader') {
      const url = generateShaderDisplacementMap(800, 60);
      setShaderMapUrl(url);
    }
  }, [mode]);

  if (!enabled) return null;

  const mapHref = getMap(mode, shaderMapUrl);
  const modeSign = mode === 'shader' ? 1 : -1;

  return (
    <div
      className={`lg-mask-overlay ${className}`}
      style={OVERLAY_STYLE}
    >
      <svg
        style={SVG_STYLE}
        aria-hidden="true"
      >
        <defs>
          <filter
            id={FILTER_ID}
            x="-35%" y="-35%" width="170%" height="170%"
            colorInterpolationFilters="sRGB"
          >
            <feImage
              x="0" y="0" width="100%" height="100%"
              result="DISPLACEMENT_MAP"
              href={mapHref}
              preserveAspectRatio="xMidYMid slice"
            />
            <feColorMatrix
              in="DISPLACEMENT_MAP"
              type="matrix"
              values="0.3 0.3 0.3 0 0
                     0.3 0.3 0.3 0 0
                     0.3 0.3 0.3 0 0
                     0 0 0 1 0"
              result="EDGE_INTENSITY"
            />
            <feComponentTransfer in="EDGE_INTENSITY" result="EDGE_MASK">
              <feFuncA type="discrete" tableValues={`0 ${aberrationIntensity * 0.05} 1`} />
            </feComponentTransfer>
            <feOffset in="SourceGraphic" dx="0" dy="0" result="CENTER_ORIGINAL" />
            <feDisplacementMap
              in="SourceGraphic" in2="DISPLACEMENT_MAP"
              scale={displacementScale * modeSign}
              xChannelSelector="R" yChannelSelector="B"
              result="RED_DISPLACED"
            />
            <feColorMatrix
              in="RED_DISPLACED" type="matrix"
              values="1 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 1 0"
              result="RED_CHANNEL"
            />
            <feDisplacementMap
              in="SourceGraphic" in2="DISPLACEMENT_MAP"
              scale={displacementScale * (modeSign - aberrationIntensity * 0.05)}
              xChannelSelector="R" yChannelSelector="B"
              result="GREEN_DISPLACED"
            />
            <feColorMatrix
              in="GREEN_DISPLACED" type="matrix"
              values="0 0 0 0 0  0 1 0 0 0  0 0 0 0 0  0 0 0 1 0"
              result="GREEN_CHANNEL"
            />
            <feDisplacementMap
              in="SourceGraphic" in2="DISPLACEMENT_MAP"
              scale={displacementScale * (modeSign - aberrationIntensity * 0.1)}
              xChannelSelector="R" yChannelSelector="B"
              result="BLUE_DISPLACED"
            />
            <feColorMatrix
              in="BLUE_DISPLACED" type="matrix"
              values="0 0 0 0 0  0 0 0 0 0  0 0 1 0 0  0 0 0 1 0"
              result="BLUE_CHANNEL"
            />
            <feBlend in="GREEN_CHANNEL" in2="BLUE_CHANNEL" mode="screen" result="GB_COMBINED" />
            <feBlend in="RED_CHANNEL" in2="GB_COMBINED" mode="screen" result="RGB_COMBINED" />
            <feGaussianBlur
              in="RGB_COMBINED"
              stdDeviation={Math.max(0.1, 0.5 - aberrationIntensity * 0.1)}
              result="ABERRATED_BLURRED"
            />
            <feComposite in="ABERRATED_BLURRED" in2="EDGE_MASK" operator="in" result="EDGE_ABERRATION" />
            <feComponentTransfer in="EDGE_MASK" result="INVERTED_MASK">
              <feFuncA type="table" tableValues="1 0" />
            </feComponentTransfer>
            <feComposite in="CENTER_ORIGINAL" in2="INVERTED_MASK" operator="in" result="CENTER_CLEAN" />
            <feComposite in="EDGE_ABERRATION" in2="CENTER_CLEAN" operator="over" />
          </filter>
        </defs>
      </svg>
      <div
        style={{
          position: 'absolute',
          inset: 0,
          filter: `url(#${FILTER_ID})`,
          backdropFilter: `blur(${(overLight ? 12 : 4) + blurAmount * 32}px) saturate(${saturation}%)`,
          WebkitBackdropFilter: `blur(${(overLight ? 12 : 4) + blurAmount * 32}px) saturate(${saturation}%)`,
          background: overLight
            ? 'linear-gradient(180deg, rgba(255, 255, 255, 0.15) 0%, rgba(255, 255, 255, 0.08) 100%)'
            : 'linear-gradient(180deg, rgba(255, 255, 255, 0.08) 0%, rgba(255, 255, 255, 0.04) 100%)',
          transition: 'backdrop-filter 0.35s cubic-bezier(0, 0, 0.6, 1)',
        }}
      />
    </div>
  );
});

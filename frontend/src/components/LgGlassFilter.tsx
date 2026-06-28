import { useState, useEffect, memo } from 'react';
import type { LgCategory } from '../App';
import { ShaderDisplacementGenerator, fragmentShaders } from '@liquid-glass/shader-utils';
import { displacementMap, polarDisplacementMap, prominentDisplacementMap } from '@liquid-glass/utils';

function getMap(mode: string, shaderMapUrl?: string): string {
  if (mode === 'shader' && shaderMapUrl) return shaderMapUrl;
  if (mode === 'polar') return polarDisplacementMap;
  if (mode === 'prominent') return prominentDisplacementMap;
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

interface Props {
  filterId: string;
  config: LgCategory;
  /** Size for shader map generation */
  shaderSize?: { width: number; height: number };
  /** Scale override for the displacement (e.g. reduced for small elements like buttons) */
  scaleOverride?: number;
}

/**
 * Shared SVG filter for liquid glass displacement + chromatic aberration.
 * Includes radial gradient edge mask and shader mode support.
 */
export const LgGlassFilter = memo(function LgGlassFilter({
  filterId,
  config,
  shaderSize = { width: 400, height: 80 },
  scaleOverride,
}: Props) {
  const { mode, displacementScale, aberrationIntensity, overLight } = config;
  const [shaderMapUrl, setShaderMapUrl] = useState('');

  useEffect(() => {
    if (mode === 'shader') {
      const url = generateShaderDisplacementMap(shaderSize.width, shaderSize.height);
      setShaderMapUrl(url);
    }
  }, [mode, shaderSize.width, shaderSize.height]);

  const mapHref = getMap(mode, shaderMapUrl);
  const baseScale = scaleOverride ?? displacementScale;
  const disp = overLight ? baseScale * 0.5 : baseScale;
  const mSign = mode === 'shader' ? 1 : -1;
  const edgeMaskOffset = Math.max(30, 80 - aberrationIntensity * 2);

  return (
    <svg style={{ position: 'absolute', inset: 0, width: '100%', height: '100%', pointerEvents: 'none' }} aria-hidden="true">
      <defs>
        {/* Radial gradient edge mask — keeps center clean, aberration only at edges */}
        <radialGradient id={`${filterId}-edge-mask`} cx="50%" cy="50%" r="50%">
          <stop offset="0%" stopColor="black" stopOpacity="0" />
          <stop offset={`${edgeMaskOffset}%`} stopColor="black" stopOpacity="0" />
          <stop offset="100%" stopColor="white" stopOpacity="1" />
        </radialGradient>

        <filter id={filterId} x="-35%" y="-35%" width="170%" height="170%" colorInterpolationFilters="sRGB">
          {/* Displacement map */}
          <feImage x="0" y="0" width="100%" height="100%" result="DISPLACEMENT_MAP"
            href={mapHref} preserveAspectRatio="xMidYMid slice" />

          {/* Edge intensity from displacement map */}
          <feColorMatrix in="DISPLACEMENT_MAP" type="matrix"
            values="0.3 0.3 0.3 0 0  0.3 0.3 0.3 0 0  0.3 0.3 0.3 0 0  0 0 0 1 0"
            result="EDGE_INTENSITY" />
          <feComponentTransfer in="EDGE_INTENSITY" result="EDGE_MASK">
            <feFuncA type="discrete" tableValues={`0 ${aberrationIntensity * 0.05} 1`} />
          </feComponentTransfer>

          {/* Original undisplaced center */}
          <feOffset in="SourceGraphic" dx="0" dy="0" result="CENTER_ORIGINAL" />

          {/* Red channel displacement */}
          <feDisplacementMap in="SourceGraphic" in2="DISPLACEMENT_MAP" scale={disp * mSign}
            xChannelSelector="R" yChannelSelector="B" result="RED_DISPLACED" />
          <feColorMatrix in="RED_DISPLACED" type="matrix"
            values="1 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 1 0" result="RED_CHANNEL" />

          {/* Green channel displacement */}
          <feDisplacementMap in="SourceGraphic" in2="DISPLACEMENT_MAP"
            scale={disp * (mSign - aberrationIntensity * 0.05)}
            xChannelSelector="R" yChannelSelector="B" result="GREEN_DISPLACED" />
          <feColorMatrix in="GREEN_DISPLACED" type="matrix"
            values="0 0 0 0 0  0 1 0 0 0  0 0 0 0 0  0 0 0 1 0" result="GREEN_CHANNEL" />

          {/* Blue channel displacement */}
          <feDisplacementMap in="SourceGraphic" in2="DISPLACEMENT_MAP"
            scale={disp * (mSign - aberrationIntensity * 0.1)}
            xChannelSelector="R" yChannelSelector="B" result="BLUE_DISPLACED" />
          <feColorMatrix in="BLUE_DISPLACED" type="matrix"
            values="0 0 0 0 0  0 0 0 0 0  0 0 1 0 0  0 0 0 1 0" result="BLUE_CHANNEL" />

          {/* Combine channels with screen blend */}
          <feBlend in="GREEN_CHANNEL" in2="BLUE_CHANNEL" mode="screen" result="GB_COMBINED" />
          <feBlend in="RED_CHANNEL" in2="GB_COMBINED" mode="screen" result="RGB_COMBINED" />

          {/* Soften aberration */}
          <feGaussianBlur in="RGB_COMBINED"
            stdDeviation={Math.max(0.1, 0.5 - aberrationIntensity * 0.1)} result="ABERRATED_BLURRED" />

          {/* Apply edge mask to aberration */}
          <feComposite in="ABERRATED_BLURRED" in2="EDGE_MASK" operator="in" result="EDGE_ABERRATION" />

          {/* Inverted mask for clean center */}
          <feComponentTransfer in="EDGE_MASK" result="INVERTED_MASK">
            <feFuncA type="table" tableValues="1 0" />
          </feComponentTransfer>
          <feComposite in="CENTER_ORIGINAL" in2="INVERTED_MASK" operator="in" result="CENTER_CLEAN" />

          {/* Final composite: edge aberration over clean center */}
          <feComposite in="EDGE_ABERRATION" in2="CENTER_CLEAN" operator="over" />
        </filter>
      </defs>
    </svg>
  );
});

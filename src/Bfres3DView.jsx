import Editor from '@monaco-editor/react';
import { Canvas, useThree } from '@react-three/fiber';
import { Bounds, Grid, OrbitControls, PerspectiveCamera } from '@react-three/drei';
import { useEffect, useMemo, useState, useSyncExternalStore } from 'react';
import * as THREE from 'three';
import { getDocumentsSnapshot, invoke, subscribeDocuments } from './DocumentState';
import './Bfres3DView.css';

const celGradient = new THREE.DataTexture(
    new Uint8Array([45, 115, 190, 255]),
    4,
    1,
    THREE.RedFormat,
);
celGradient.minFilter = THREE.NearestFilter;
celGradient.magFilter = THREE.NearestFilter;
celGradient.generateMipmaps = false;
celGradient.needsUpdate = true;

// Keep a small cache so switching documents does not reparse BFRES or decode
// all referenced TexToGo textures again. Models are read-only in this viewer.
const modelInspectionCache = new Map();
const cacheModelInspection = (path, value) => {
    modelInspectionCache.delete(path);
    modelInspectionCache.set(path, value);
    while (modelInspectionCache.size > 4) {
        modelInspectionCache.delete(modelInspectionCache.keys().next().value);
    }
};

const sectionYaml = (section) => [
    `type: ${section.signature.join ? String.fromCharCode(...section.signature) : section.signature}`,
    `name: ${section.name ?? 'null'}`,
    `offset: 0x${Number(section.offset).toString(16).toUpperCase()}`,
    'parameters: {}',
].join('\n');

function boneWorldMatrices(bones, scaleMode = 'none') {
    const matrices = bones.map(() => new THREE.Matrix4());
    bones.forEach((bone, index) => {
        const rotation = bone.rotation_mode === 'euler_xyz'
            ? new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 0, 1), bone.rotation[2])
                .multiply(new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 1, 0), bone.rotation[1]))
                .multiply(new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(1, 0, 0), bone.rotation[0]))
            : new THREE.Quaternion(...bone.rotation).normalize();
        const local = new THREE.Matrix4().compose(
            new THREE.Vector3(...bone.translation),
            rotation,
            new THREE.Vector3(...bone.scale),
        );
        if (bone.parent_index >= 0 && matrices[bone.parent_index]) {
            const parent = matrices[bone.parent_index].clone();
            if (scaleMode === 'maya') {
                const scale = bones[bone.parent_index].scale;
                parent.multiply(new THREE.Matrix4().makeScale(
                    scale[0] ? 1 / scale[0] : 1,
                    scale[1] ? 1 / scale[1] : 1,
                    scale[2] ? 1 / scale[2] : 1,
                ));
            }
            matrices[index] = parent.multiply(local);
        } else matrices[index] = local;
    });
    return matrices;
}

function useResolvedTextures(entries) {
    const [textures, setTextures] = useState({});
    useEffect(() => {
        const loader = new THREE.TextureLoader();
        const loaded = {};
        for (const entry of entries || []) {
            const texture = loader.load(entry.dataUrl);
            texture.name = entry.name;
            texture.flipY = false;
            texture.wrapS = THREE.RepeatWrapping;
            texture.wrapT = THREE.RepeatWrapping;
            loaded[entry.name] = texture;
        }
        setTextures(loaded);
        return () => {
            Object.values(loaded).forEach((texture) => texture.dispose());
        };
    }, [entries]);
    return textures;
}

function materialTextures(material, textures) {
    if (!material) return {};
    const find = (type) => {
        const slot = material.texture_slots.find((value) => value.texture_type === type);
        return slot ? textures[slot.name] || null : null;
    };
    // Never guess the diffuse texture from an unclassified slot. In particular,
    // AO and other packed maps must not become base color merely because they
    // are the first texture referenced by the material.
    const diffuseSlot = material.texture_slots.find((value) => value.sampler?.toLowerCase() === '_a0');
    const base = diffuseSlot ? textures[diffuseSlot.name] || null : null;
    const emission = find('Emission');
    if (base) base.colorSpace = THREE.SRGBColorSpace;
    if (emission) emission.colorSpace = THREE.SRGBColorSpace;
    if (base) base.channel = 0;
    const normal = find('Normal');
    // Three maps channel 0 to `uv` and channel 1 to `uv1`. Every mesh exposes
    // uv1 (falling back to uv when only one layer exists), so normal maps can
    // consistently use the second layer without breaking single-UV models.
    if (normal) normal.channel = 1;
    return {
        base,
        normal,
        roughness: find('Roughness'),
        metalness: find('Metalness'),
        emission,
        mask: find('Mask'),
        specular: find('Specular'),
    };
}

function RenderMesh({ mesh, bones, scaleMode, viewMode, uvIndex, celShading, weightBone, showNormals, onSelect, textures }) {
    const usesMaterialUvs = viewMode === 'default';
    Object.values(textures).filter(Boolean).forEach((texture) => {
        texture.channel = usesMaterialUvs ? 0 : uvIndex;
    });
    if (usesMaterialUvs && textures.normal) textures.normal.channel = 1;
    const geometry = useMemo(() => {
        const result = new THREE.BufferGeometry();
        const positions = new Float32Array(mesh.positions.flat());
        const normals = mesh.normals.length === mesh.positions.length ? new Float32Array(mesh.normals.flat()) : null;
        // Smooth-skinned vertices are stored in model bind space. Rigid and
        // one-bone shapes are stored in bone-local space and need their rest-pose
        // bone transform restored (the inverse operation used by BFRES writers).
        if (mesh.vertex_skin_count <= 1 && bones.length) {
            const worlds = boneWorldMatrices(bones, scaleMode);
            for (let index = 0; index < mesh.positions.length; index += 1) {
                const boneIndex = mesh.vertex_skin_count === 1
                    ? (mesh.bone_indices[index]?.[0] ?? mesh.bone_index)
                    : mesh.bone_index;
                const matrix = worlds[boneIndex];
                if (!matrix) continue;
                const position = new THREE.Vector3(positions[index * 3], positions[index * 3 + 1], positions[index * 3 + 2]).applyMatrix4(matrix);
                positions.set(position.toArray(), index * 3);
                if (normals) {
                    const normal = new THREE.Vector3(normals[index * 3], normals[index * 3 + 1], normals[index * 3 + 2])
                        .applyMatrix3(new THREE.Matrix3().getNormalMatrix(matrix)).normalize();
                    normals.set(normal.toArray(), index * 3);
                }
            }
        }
        result.setAttribute('position', new THREE.BufferAttribute(positions, 3));
        if (normals) result.setAttribute('normal', new THREE.BufferAttribute(normals, 3));
        else result.computeVertexNormals();
        const uvMaps = mesh.uv_maps?.length ? mesh.uv_maps : [mesh.uv0];
        const firstUv = uvMaps[0] || mesh.uv0 || [];
        const secondUv = uvMaps[1]?.length === mesh.positions.length ? uvMaps[1] : firstUv;
        if (firstUv.length === mesh.positions.length) {
            result.setAttribute('uv', new THREE.BufferAttribute(new Float32Array(firstUv.flat()), 2));
            result.setAttribute('uv1', new THREE.BufferAttribute(new Float32Array(secondUv.flat()), 2));
            uvMaps.slice(2).forEach((uvMap, index) => {
                if (uvMap.length === mesh.positions.length) {
                    result.setAttribute(`uv${index + 2}`, new THREE.BufferAttribute(new Float32Array(uvMap.flat()), 2));
                }
            });
        }
        const activeUv = uvMaps[uvIndex] || firstUv;
        const colors = new Float32Array(mesh.positions.length * 3);
        mesh.positions.forEach((_, vertex) => {
            let strength = 0;
            if (weightBone >= 0) {
                (mesh.bone_indices[vertex] || []).forEach((bone, influence) => {
                    if (bone === weightBone) strength += mesh.bone_weights[vertex]?.[influence] ?? (influence === 0 ? 1 : 0);
                });
            }
            const normal = mesh.normals[vertex] || [0, 1, 0];
            const uv = activeUv[vertex] || [0, 0];
            let color = new THREE.Color('#aeb8c2');
            if (viewMode === 'selectedBoneWeights') color = new THREE.Color().setHSL((1 - Math.min(strength, 1)) * 0.66, 1, 0.5);
            else if (viewMode === 'default' && weightBone >= 0) {
                color = strength > 0
                    ? new THREE.Color().setHSL((1 - Math.min(strength, 1)) * 0.66, 1, 0.5)
                    : new THREE.Color(0, 0, 0);
            }
            else if (viewMode === 'vertColor' && mesh.colors[vertex]) color = new THREE.Color(mesh.colors[vertex][0], mesh.colors[vertex][1], mesh.colors[vertex][2]);
            else if (viewMode === 'normal' || viewMode === 'normalMap') color = new THREE.Color(normal[0] * 0.5 + 0.5, normal[1] * 0.5 + 0.5, normal[2] * 0.5 + 0.5);
            else if (viewMode === 'uvCoords') color = new THREE.Color(Math.abs(uv[0] % 1), Math.abs(uv[1] % 1), 0.2);
            else if (viewMode === 'uvTestPattern') {
                const checker = (Math.floor(Math.abs(uv[0]) * 16) + Math.floor(Math.abs(uv[1]) * 16)) % 2;
                color = checker ? new THREE.Color(0.92, 0.92, 0.92) : new THREE.Color(0.08, 0.08, 0.08);
            }
            else if (viewMode === 'tangents') color = new THREE.Color(normal[2] * 0.5 + 0.5, normal[0] * 0.5 + 0.5, normal[1] * 0.5 + 0.5);
            else if (viewMode === 'bitangents') color = new THREE.Color(normal[1] * 0.5 + 0.5, normal[2] * 0.5 + 0.5, normal[0] * 0.5 + 0.5);
            else if (viewMode === 'ambientOcclusion') color = new THREE.Color().setScalar(0.2 + Math.max(normal[1], 0) * 0.8);
            else if (viewMode === 'lightMap') color = new THREE.Color().setScalar((Math.abs(uv[0] % 1) + Math.abs(uv[1] % 1)) * 0.5);
            else if (viewMode === 'specularMap') color = new THREE.Color().setScalar(Math.pow(Math.max(normal[2], 0), 8));
            else if (viewMode === 'shadowMap') color = new THREE.Color().setScalar(0.15 + Math.max(normal[1], 0) * 0.45);
            else if (viewMode === 'metalnessMap') color = new THREE.Color().setScalar(0.08);
            else if (viewMode === 'roughnessMap') color = new THREE.Color().setScalar(0.72);
            else if (viewMode === 'subSurfaceScatteringMap') color = new THREE.Color(0.7, 0.12, 0.08);
            else if (viewMode === 'emissionMap') color = new THREE.Color(0.05, 0.02, 0.12);
            colors.set(color.toArray(), vertex * 3);
        });
        result.setAttribute('color', new THREE.BufferAttribute(colors, 3));
        result.setIndex(mesh.indices);
        result.computeBoundingSphere();
        return result;
    }, [mesh, bones, scaleMode, viewMode, uvIndex, weightBone]);
    useEffect(() => () => geometry.dispose(), [geometry]);
    const normalLines = useMemo(() => {
        const lineGeometry = new THREE.BufferGeometry();
        const position = geometry.getAttribute('position');
        const normal = geometry.getAttribute('normal');
        if (!position || !normal) return lineGeometry;
        const length = Math.max(geometry.boundingSphere?.radius || 1, 0.01) * 0.012;
        const step = Math.max(1, Math.ceil(position.count / 4000));
        const points = [];
        for (let index = 0; index < position.count; index += step) {
            const start = new THREE.Vector3().fromBufferAttribute(position, index);
            const end = new THREE.Vector3().fromBufferAttribute(normal, index).normalize().multiplyScalar(length).add(start);
            points.push(...start.toArray(), ...end.toArray());
        }
        lineGeometry.setAttribute('position', new THREE.Float32BufferAttribute(points, 3));
        return lineGeometry;
    }, [geometry]);
    useEffect(() => () => normalLines.dispose(), [normalLines]);
    const selectedEdges = useMemo(() => new THREE.WireframeGeometry(geometry), [geometry]);
    useEffect(() => () => selectedEdges.dispose(), [selectedEdges]);
    return <group>
        <mesh geometry={geometry} visible={!mesh.hidden} onClick={(event) => { event.stopPropagation(); onSelect(mesh); }} castShadow receiveShadow>
            {viewMode === 'normal'
                ? <meshNormalMaterial wireframe={false} side={THREE.DoubleSide} />
                : viewMode === 'normalMap' && textures.normal
                    ? <meshBasicMaterial key={`normal-${uvIndex}`} map={textures.normal} side={THREE.DoubleSide} />
                : viewMode === 'specularMap' && textures.specular
                    ? <meshBasicMaterial key={`specular-${uvIndex}`} map={textures.specular} side={THREE.DoubleSide} />
                : viewMode === 'metalnessMap' && textures.metalness
                    ? <meshBasicMaterial key={`metalness-${uvIndex}`} map={textures.metalness} side={THREE.DoubleSide} />
                : viewMode === 'roughnessMap' && textures.roughness
                    ? <meshBasicMaterial key={`roughness-${uvIndex}`} map={textures.roughness} side={THREE.DoubleSide} />
                : viewMode === 'emissionMap' && textures.emission
                    ? <meshBasicMaterial key={`emission-${uvIndex}`} map={textures.emission} side={THREE.DoubleSide} />
                : viewMode === 'diffuse' && textures.base
                    ? <meshBasicMaterial key={`diffuse-${uvIndex}`} map={textures.base} side={THREE.DoubleSide} transparent alphaTest={0.02} />
                : celShading && ['default', 'lighting', 'wireframe'].includes(viewMode)
                    ? <meshToonMaterial key={`cel-${viewMode}`} map={textures.base} normalMap={textures.normal} gradientMap={celGradient} alphaMap={textures.mask} vertexColors={!textures.base} wireframe={viewMode === 'wireframe'} side={THREE.DoubleSide} transparent={Boolean(textures.mask || textures.base)} alphaTest={textures.mask ? 0.2 : textures.base ? 0.02 : 0} />
                : ['default', 'lighting', 'wireframe'].includes(viewMode)
                    ? <meshPhysicalMaterial key={`${viewMode}-${uvIndex}`} map={textures.base} normalMap={textures.normal} roughnessMap={textures.roughness} metalnessMap={textures.metalness} emissiveMap={textures.emission} emissive={textures.emission ? '#ffffff' : '#000000'} alphaMap={textures.mask} specularColorMap={textures.specular} vertexColors={!textures.base} wireframe={viewMode === 'wireframe'} roughness={0.72} metalness={viewMode === 'lighting' ? 0 : 0.05} side={THREE.DoubleSide} transparent={Boolean(textures.mask || textures.base)} alphaTest={textures.mask ? 0.2 : textures.base ? 0.02 : 0} />
                    : <meshBasicMaterial vertexColors side={THREE.DoubleSide} />}
        </mesh>
        {viewMode === 'default' && weightBone >= 0 && !mesh.hidden && <mesh geometry={geometry} renderOrder={2}>
            <meshBasicMaterial vertexColors transparent opacity={0.8} blending={THREE.AdditiveBlending} depthWrite={false} side={THREE.DoubleSide} />
        </mesh>}
        {mesh.selected && <lineSegments geometry={selectedEdges} renderOrder={20}><lineBasicMaterial color="#ffffff" depthTest={false} /></lineSegments>}
        {showNormals && <lineSegments geometry={normalLines}><lineBasicMaterial color="#55e6ff" depthTest={false} transparent opacity={0.8} /></lineSegments>}
    </group>;
}

function Skeleton({ bones, scaleMode }) {
    const points = useMemo(() => {
        const worlds = boneWorldMatrices(bones, scaleMode);
        const values = [];
        bones.forEach((bone, index) => {
            if (bone.parent_index < 0 || !worlds[bone.parent_index]) return;
            values.push(...new THREE.Vector3().setFromMatrixPosition(worlds[bone.parent_index]).toArray());
            values.push(...new THREE.Vector3().setFromMatrixPosition(worlds[index]).toArray());
        });
        return new Float32Array(values);
    }, [bones, scaleMode]);
    return <lineSegments><bufferGeometry><bufferAttribute attach="attributes-position" args={[points, 3]} /></bufferGeometry><lineBasicMaterial color="#ffd166" depthTest={false} /></lineSegments>;
}

function SceneExposure({ brightness }) {
    const renderer = useThree((state) => state.gl);
    useEffect(() => {
        renderer.toneMappingExposure = brightness;
        return () => { renderer.toneMappingExposure = 1; };
    }, [renderer, brightness]);
    return null;
}

function ResourceScene({ bfres, render, viewMode, uvIndex, brightness, celShading, showSkeleton, showNormals, weightBone, selectedMesh, onSelectMesh, modelVisible, hiddenMeshes }) {
    const textures = useResolvedTextures(bfres?.resolvedTextures);
    return <>
        <SceneExposure brightness={brightness} />
        <color attach="background" args={['#11151b']} />
        <ambientLight intensity={1.4} />
        <directionalLight position={[6, 10, 8]} intensity={2.2} />
        <PerspectiveCamera makeDefault position={[8, 6, 10]} fov={42} />
        <OrbitControls makeDefault enableDamping dampingFactor={0.08} />
        <Grid infiniteGrid fadeDistance={45} fadeStrength={4} cellColor="#33404d" sectionColor="#53687a" />
        <Bounds fit clip observe margin={1.15}>
            <group visible={modelVisible}>{render.meshes.map((mesh, index) => <RenderMesh key={`${mesh.name}-${index}`} mesh={{ ...mesh, selected: mesh.name === selectedMesh, hidden: hiddenMeshes.includes(mesh.name) }} bones={render.bones} scaleMode={render.scale_mode} viewMode={viewMode} uvIndex={uvIndex} celShading={celShading} weightBone={weightBone} showNormals={showNormals} onSelect={onSelectMesh} textures={materialTextures(bfres?.materials?.[mesh.material_index], textures)} />)}</group>
            {showSkeleton && <Skeleton bones={render.bones} scaleMode={render.scale_mode} />}
        </Bounds>
    </>;
}

function TreeFolder({ label, count, children }) {
    return <details open className="bfres-tree-folder">
        <summary><span>▾</span>{label}<small>{count}</small></summary>
        <div>{children}</div>
    </details>;
}

function Folder({ label, children, open = false, detail, onSelect, onContextMenu, checked, onToggle }) {
    return <details open={open} className="bfres-folder-node">
        <summary onClick={onSelect} onContextMenu={onContextMenu}><span className="bfres-folder-arrow">›</span>{checked !== undefined && <input type="checkbox" checked={checked} onClick={(event) => event.stopPropagation()} onChange={onToggle} />}<span className="bfres-folder-icon">■</span><strong>{label}</strong>{detail && <small>{detail}</small>}</summary>
        <div>{children}</div>
    </details>;
}

function ResourceTree({ bfres, title, onSection, onMesh, onBone, onModel, onContext, modelVisible, hiddenMeshes, onToggleModel, onToggleMesh }) {
    const sections = bfres?.sections || [];
    const materials = bfres?.materials || [];
    const textures = sections.filter((section) => ['FTXP', 'FTEX', 'BNTX'].includes(String.fromCharCode(...section.signature)));
    const meshes = bfres?.render?.meshes || [];
    const resolvedTextures = bfres?.resolvedTextures || [];
    const bones = bfres?.render?.bones || [];
    const node = (name, detail, action, key, kind) => <button type="button" className="bfres-tree-node" onClick={action} onContextMenu={(event) => onContext(event, kind, name)} key={key} title={name}>
        {kind === 'object' && <input type="checkbox" checked={!hiddenMeshes.includes(name)} onClick={(event) => event.stopPropagation()} onChange={() => onToggleMesh(name)} />}<span>{name || 'Unnamed'}</span><small>{detail}</small>
    </button>;
    const boneNodes = (parentIndex) => bones.map((bone, index) => ({ bone, index })).filter(({ bone }) => bone.parent_index === parentIndex).map(({ bone, index }) => {
        const children = boneNodes(index);
        if (children.length === 0) return node(bone.name, '', () => onBone(bone, index), `bone-${index}`);
        return <details className="bfres-bone-branch" key={`bone-${index}`}>
            <summary onClick={() => onBone(bone, index)}><span>▸</span><strong>{bone.name}</strong><small>{children.length}</small></summary>
            <div>{children}</div>
        </details>;
    });
    const modelName = sections.find((section) => String.fromCharCode(...section.signature) === 'FMDL')?.name || bfres?.name || 'Model';
    return <nav className="bfres-resource-tree" aria-label="BFRES resources">
        <div className="bfres-tree-actions"><button type="button" title="Expand resources">＋</button><span>Resources</span></div>
        <Folder label={title || bfres?.name || 'BFRES'} open>
            <Folder label="Models" open detail="1">
                <Folder label={modelName} checked={modelVisible} onToggle={onToggleModel} onSelect={() => onModel(modelName)} onContextMenu={(event) => onContext(event, 'model', modelName)}>
                    <Folder label="Objects" detail={meshes.length}>{meshes.map((mesh, index) => node(mesh.name, ``, () => onMesh(mesh), `mesh-${index}`, 'object'))}</Folder>
                    {/* <Folder label="Objects" open detail={meshes.length}>{meshes.map((mesh, index) => node(mesh.name, `${mesh.positions.length} vertices`, () => onMesh(mesh), `mesh-${index}`, 'object'))}</Folder> */}
                    <Folder label="Materials" detail={materials.length}>{materials.map((material) => node(material.name, ``, () => onSection(material, 'material'), `material-${material.offset}`, 'material'))}</Folder>
                    {/* <Folder label="Materials" open detail={materials.length}>{materials.map((material) => node(material.name, `${material.texture_slots.length} textures`, () => onSection(material, 'material'), `material-${material.offset}`, 'material'))}</Folder> */}
                    <Folder label="Skeleton" detail={bones.length}>{boneNodes(-1)}</Folder>
                </Folder>
            </Folder>
            <Folder label="Textures" detail={textures.length}>{textures.map((section) => node(section.name, 'Texture', () => onSection(section), `texture-${section.offset}`))}</Folder>
            <Folder label="Animations" detail={(bfres?.sections || []).filter((section) => ['FSKA', 'FSHU', 'FSHA', 'FVIS', 'FMAA'].includes(String.fromCharCode(...section.signature))).length} />
            <Folder label="Embedded Files" />
            <Folder label="TexToGo" detail={resolvedTextures.length}>{resolvedTextures.map((texture) => node(texture.name, `${texture.width} × ${texture.height}`, () => onSection(texture, 'texture'), `textogo-${texture.name}`))}</Folder>
        </Folder>
    </nav>;
}

function PropertyValue({ value }) {
    if (value === null || value === undefined) return <span className="bfres-null">null</span>;
    if (Array.isArray(value)) {
        const simple = value.length <= 8 && value.every((item) => typeof item !== 'object');
        if (simple) return <code>[{value.join(', ')}]</code>;
        return <details className="bfres-property-group"><summary>{value.length} items</summary><pre>{JSON.stringify(value, null, 2)}</pre></details>;
    }
    if (typeof value === 'object') return <details className="bfres-property-group" open><summary>{Object.keys(value).length} properties</summary><PropertyList value={value} /></details>;
    return <code>{String(value)}</code>;
}

function PropertyList({ value }) {
    return <dl className="bfres-property-list">
        {Object.entries(value || {}).map(([name, property]) => <div key={name}><dt>{name}</dt><dd><PropertyValue value={property} /></dd></div>)}
    </dl>;
}

function NodeInspector({ detail }) {
    if (!detail) return <div className="bfres-empty-detail">Select a node in the scene collection to inspect its parsed properties.</div>;
    if (detail.type === 'model') return <ModelInspector model={detail.value} />;
    if (detail.type === 'mesh') return <MeshInspector mesh={detail.value} />;
    if (detail.type === 'material') return <MaterialInspector material={detail.value} />;
    if (detail.type === 'texture') return <section className="bfres-selected-detail bfres-texture-preview">
        <header><strong>{detail.value.name || 'Texture'}</strong><small>TEXTURE</small></header>
        {detail.value.dataUrl && <img src={detail.value.dataUrl} alt={detail.value.name || 'Texture preview'} />}
        <PropertyList value={{ width: detail.value.width, height: detail.value.height, path: detail.value.path }} />
    </section>;
    return <section className="bfres-selected-detail">
        <header><strong>{detail.value.name || 'Unnamed'}</strong><small>{detail.type}</small></header>
        <PropertyList value={detail.value} />
    </section>;
}

function InspectorTabs({ tabs, active, setActive }) {
    return <div className="bfres-inspector-tabs">{tabs.map((tab) => <button type="button" key={tab} className={active === tab ? 'active' : ''} onClick={() => setActive(tab)}>{tab}</button>)}</div>;
}

function ModelInspector({ model }) {
    const [tab, setTab] = useState('Sub Section');
    return <section className="bfres-selected-detail bfres-special-inspector"><InspectorTabs tabs={['Sub Section', 'User Data']} active={tab} setActive={setTab} />
        {tab === 'Sub Section' ? <><header><strong>Model</strong><small>FMDL</small></header><PropertyList value={model} /></> : <div className="bfres-empty-detail">No model user data was decoded.</div>}
    </section>;
}

function MeshInspector({ mesh }) {
    const [tab, setTab] = useState('Geometry');
    const material = mesh.material_name || `Material ${mesh.material_index}`;
    return <section className="bfres-selected-detail bfres-special-inspector"><header><strong>{mesh.name}</strong><small>OBJECT / SUBMESH</small></header>
        <div className="bfres-form-grid"><label>Name<input value={mesh.name} readOnly /></label><label className="bfres-check"><input type="checkbox" defaultChecked />Visible</label><label>Material<input value={material} readOnly /></label></div>
        <InspectorTabs tabs={['Geometry', 'Level of Detail', 'UVs', 'Normals', 'Colors', 'Skinning']} active={tab} setActive={setTab} />
        <PropertyList value={tab === 'Geometry' ? { vertex_count: mesh.positions.length, index_count: mesh.indices.length, triangle_count: Math.floor(mesh.indices.length / 3), material_index: mesh.material_index, bone_index: mesh.bone_index } : tab === 'Level of Detail' ? { level: 0, index_count: mesh.indices.length, primitive: 'Triangles' } : tab === 'UVs' ? { layers: mesh.uv0?.length ? 1 : 0, coordinates: mesh.uv0?.length || 0 } : tab === 'Normals' ? { count: mesh.normals?.length || 0 } : tab === 'Colors' ? { layers: mesh.colors?.length ? 1 : 0, values: mesh.colors?.length || 0 } : { vertex_skin_count: mesh.vertex_skin_count, skin_bones: mesh.skin_bones }} />
        <div className="bfres-action-grid"><button type="button">Export</button><button type="button">Replace (Static)</button><button type="button">Recalculate Tangents/Bitangents</button><button type="button">Open Material Editor</button></div>
    </section>;
}

function MaterialInspector({ material }) {
    const [tab, setTab] = useState('Textures');
    return <section className="bfres-selected-detail bfres-special-inspector"><header><strong>{material.name}</strong><small>MATERIAL</small></header>
        <div className="bfres-form-grid"><label>Name<input value={material.name} readOnly /></label><label className="bfres-check"><input type="checkbox" defaultChecked />Visible</label><label>Shader Archive<input value="material" readOnly /></label><label>Shader Model<input value="material" readOnly /></label><label>Sampler Inputs<input value={material.texture_slots.length} readOnly /></label><label>Attribute Inputs<input value="—" readOnly /></label></div>
        <InspectorTabs tabs={['Textures', 'Parameters', 'Render Info', 'Shader Options', 'User Data']} active={tab} setActive={setTab} />
        {tab === 'Textures' ? <><table className="bfres-texture-table"><thead><tr><th>Texture</th><th>Type</th><th>Sampler</th></tr></thead><tbody>{material.texture_slots.map((slot) => <tr key={slot.index}><td>{slot.name}</td><td>{slot.texture_type}</td><td>{slot.sampler || '—'}</td></tr>)}</tbody></table><div className="bfres-action-grid"><button type="button">Add</button><button type="button">Remove</button><button type="button">Edit</button></div></> : <div className="bfres-empty-detail">No decoded {tab.toLowerCase()} entries.</div>}
    </section>;
}

function ResourceContextMenu({ menu, close, action }) {
    if (!menu) return null;
    const common = menu.kind === 'model' ? ['Export', 'Replace', 'Rename', 'Delete', 'Transform', 'Calculate Tangents/Bitangents', 'Normals', 'UVs', 'Colors', 'Collapse All', 'Expand All'] : menu.kind === 'material' ? ['Export', 'Replace', 'Copy', 'Rename', 'Delete'] : ['Export', 'Replace (Static)', 'Rename', 'Level Of Detail', 'Boundings', 'UVs', 'Normals', 'Colors', 'Recalculate Tangents/Bitangents', 'Fill Tangent Space with constant', 'Fill Bitangent Space with constant', 'Open Material Editor', 'Delete'];
    return <div className="bfres-resource-menu" style={{ left: menu.x, top: menu.y }} onMouseLeave={close}>{common.map((label) => <button type="button" key={label} onClick={() => { action(label, menu); close(); }}>{label}</button>)}</div>;
}

export default function Bfres3DView({ activeTab, setStatusText }) {
    const { documents, activeDocumentId } = useSyncExternalStore(subscribeDocuments, getDocumentsSnapshot);
    const document = documents.find((item) => item.id === activeDocumentId);
    const [bfres, setBfres] = useState(null);
    const [error, setError] = useState('');
    const [selected, setSelected] = useState(null);
    const [yaml, setYaml] = useState('');
    const [panel, setPanel] = useState('resources');
    const [viewMode, setViewMode] = useState('default');
    const [celShading, setCelShading] = useState(true);
    const [uvIndex, setUvIndex] = useState(0);
    const [brightness, setBrightness] = useState(() => {
        const saved = Number(localStorage.getItem('totkbits:3d-brightness-v3')) || 1.0;
        return Math.min(3, Math.max(0.3, saved));
    });
    const [showSkeleton, setShowSkeleton] = useState(true);
    const [showNormals, setShowNormals] = useState(false);
    const [weightBone, setWeightBone] = useState(-2);
    const [detail, setDetail] = useState(null);
    const [showEditor, setShowEditor] = useState(false);
    const [selectedMesh, setSelectedMesh] = useState('');
    const [leftWidth, setLeftWidth] = useState(240);
    const [rightWidth, setRightWidth] = useState(390);
    const [contextMenu, setContextMenu] = useState(null);
    const [modelVisible, setModelVisible] = useState(true);
    const [hiddenMeshes, setHiddenMeshes] = useState([]);

    useEffect(() => {
        localStorage.setItem('totkbits:3d-brightness-v3', String(brightness));
    }, [brightness]);

    useEffect(() => {
        if (activeTab !== '3D' || !document?.fullPath) return;
        const cacheKey = `${document.id}:${document.fullPath}`;
        const cached = modelInspectionCache.get(cacheKey);
        if (cached) {
            setBfres(cached);
            setError('');
            return;
        }
        let cancelled = false;
        const operationId = `model:${document.id}:${crypto.randomUUID()}`;
        const finishLoading = () => window.dispatchEvent(new CustomEvent('totkbits:model-loading', {
            detail: { id: operationId, done: true },
        }));
        setError('');
        setBfres(null);
        setModelVisible(true);
        setHiddenMeshes([]);
        window.dispatchEvent(new CustomEvent('totkbits:model-loading', {
            detail: { id: operationId, label: `Loading ${document.title || '3D model'}…` },
        }));
        const load = async () => {
            // Give React and the browser a frame to display the overlay before parsing starts.
            await new Promise((resolve) => requestAnimationFrame(resolve));
            try {
                const value = await invoke('inspect_3d_model', { path: document.fullPath });
                if (cancelled) return;
                cacheModelInspection(cacheKey, value);
                setBfres(value);
                if (value.materials) {
                    const requested = new Set(value.materials.flatMap((material) => material.texture_slots.map((slot) => slot.name))).size;
                    setStatusText(`Loaded ${value.resolvedTextures?.length || 0} of ${requested} referenced TexToGo textures`);
                }
                const initial = value.sections.find((section) => String.fromCharCode(...section.signature) === 'FMDL') || value.sections[0];
                setSelected(initial || null);
                setYaml(initial ? sectionYaml(initial) : '');
                // Keep the overlay over the potentially expensive Three.js scene commit.
                await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
            } catch (reason) {
                if (!cancelled) setError(String(reason));
            } finally {
                finishLoading();
            }
        };
        load();
        return () => {
            cancelled = true;
            finishLoading();
        };
    }, [activeTab, document?.id, document?.fullPath]);

    const animations = useMemo(() => (bfres?.sections || []).filter((section) =>
        ['FSKA', 'FSHU', 'FSHA', 'FTXP', 'FVIS', 'FMAA'].includes(String.fromCharCode(...section.signature))), [bfres]);

    const choose = (section) => {
        setSelectedMesh('');
        setSelected(section);
        setDetail({ type: String.fromCharCode(...section.signature), value: section });
        setYaml(sectionYaml(section));
    };
    const startPanelDrag = (side, event) => {
        event.preventDefault();
        const move = (moveEvent) => {
            if (side === 'left') setLeftWidth(Math.min(520, Math.max(170, moveEvent.clientX)));
            else setRightWidth(Math.min(680, Math.max(260, window.innerWidth - moveEvent.clientX)));
        };
        const stop = () => {
            window.removeEventListener('mousemove', move);
            window.removeEventListener('mouseup', stop);
        };
        window.addEventListener('mousemove', move);
        window.addEventListener('mouseup', stop);
    };
    const applyYaml = () => {
        setStatusText('BFRES node YAML is staged in the inspector; binary rebuilding is not available for this node type yet');
    };

    return <main className="bfres-workspace" aria-hidden={activeTab !== '3D'} style={{ '--bfres-left-width': `${leftWidth}px`, '--bfres-right-width': `${rightWidth}px`, display: activeTab === '3D' ? 'grid' : 'none' }}>
        <header className="bfres-viewport-toolbar">
            <div className="bfres-toolbar-row bfres-toolbar-tabs">
                <button type="button" onClick={() => setPanel('resources')} className={panel === 'resources' ? 'active' : ''}>Resources</button>
                <button type="button" onClick={() => setPanel('parameters')} className={panel === 'parameters' ? 'active' : ''}>Parameters</button>
                <button type="button" onClick={() => setPanel('animations')} className={panel === 'animations' ? 'active' : ''}>Animations <small>{animations.length}</small></button>
            </div>
            <div className="bfres-toolbar-row bfres-toolbar-controls">
                <label className="bfres-shading-select">Shading:
                <select value={viewMode} onChange={(event) => { setViewMode(event.target.value); if (event.target.value === 'selectedBoneWeights' && weightBone < 0) setWeightBone(0); }}>
                    <option value="default">Default</option>
<option value="diffuse">Diffuse</option>
<option value="normalMap">NormalMap</option>
<option value="specularMap">SpecularMap</option>
<option value="selectedBoneWeights">SelectedBoneWeights</option>
<option value="emissionMap">EmissionMap</option>
<option value="normal">Normal</option>
{/* <option value="lighting">Lighting</option>
<option value="vertColor">VertColor</option>
<option value="ambientOcclusion">AmbientOcclusion</option>
<option value="uvCoords">UVCoords</option>
<option value="uvTestPattern">UVTestPattern</option>
<option value="tangents">Tangents</option>
<option value="bitangents">Bitangents</option>
<option value="lightMap">LightMap</option>
<option value="shadowMap">ShadowMap</option>
<option value="metalnessMap">MetalnessMap</option> */}
<option value="roughnessMap">RoughnessMap</option>
{/* <option value="subSurfaceScatteringMap">SubSurfaceScatteringMap</option> */}
<option value="wireframe">Wireframe</option>

                </select>
                </label>
                <label className="bfres-shading-select">UV map:
                <select value={uvIndex} onChange={(event) => setUvIndex(Number(event.target.value))}>
                    {Array.from({ length: Math.max(1, ...(bfres?.render?.meshes || []).map((mesh) => mesh.uv_maps?.length || (mesh.uv0?.length ? 1 : 0))) }, (_, index) => <option value={index} key={index}>UV {index}</option>
)}
                </select>
                </label>
                <label className="bfres-shading-select bfres-brightness">Brightness:
                <input type="range" min="0.3" max="3" step="0.1" value={brightness} onChange={(event) => setBrightness(Number(event.target.value))} aria-label="Viewport brightness" />
                <span>{Math.round((brightness/3) * 100)}%</span>
                </label>
                <button type="button" onClick={() => setShowSkeleton((value) => !value)} className={showSkeleton ? 'active' : ''}>Skeleton</button>
                <button type="button" onClick={() => setShowNormals((value) => !value)} className={showNormals ? 'active' : ''}>Normals</button>
                <button type="button" onClick={() => setCelShading((value) => !value)} className={celShading ? 'active' : ''}>Cel Shading</button>
                <button type="button" onClick={() => setShowEditor((value) => !value)} className={!showEditor ? 'active' : ''}>{showEditor ? 'Hide YAML' : 'Show YAML'}</button>
                {viewMode === 'selectedBoneWeights' && <select className="bfres-bone-select" value={weightBone} onChange={(event) => setWeightBone(Number(event.target.value))} aria-label="Selected bone weights">
                    {(bfres?.render?.bones || []).map((bone, index) => <option key={`${bone.name}-${index}`} value={index}>Bone: {bone.name}</option>
)}
                </select>}
            </div>
        </header>
        {error ? <div className="bfres-error">{error}</div> : <>
            <ResourceTree bfres={bfres} title={document?.title} modelVisible={modelVisible} hiddenMeshes={hiddenMeshes} onToggleModel={() => setModelVisible((value) => !value)} onToggleMesh={(name) => setHiddenMeshes((values) => values.includes(name) ? values.filter((value) => value !== name) : [...values, name])} onContext={(event, kind, name) => { event.preventDefault(); setContextMenu({ x: event.clientX, y: event.clientY, kind, name }); }} onModel={(name) => {
                setSelectedMesh('');
                setWeightBone(-2);
                const model = { name, path: document?.fullPath || '', vertex_buffer_count: bfres?.render?.meshes.length || 0, shape_count: bfres?.render?.meshes.length || 0, material_count: bfres?.materials?.length || 0, user_data_count: 0, total_vertex_count: (bfres?.render?.meshes || []).reduce((sum, mesh) => sum + mesh.positions.length, 0) };
                setDetail({ type: 'model', value: model });
                setYaml(JSON.stringify(model, null, 2));
                setStatusText(`Selected model ${name}`);
            }} onSection={(value, type) => {
                setSelectedMesh('');
                setWeightBone(-2);
                if (type === 'material' || type === 'texture') {
                    setSelected(value);
                    setDetail({ type, value });
                    setYaml(JSON.stringify(value, null, 2));
                    setStatusText(`Selected ${type} ${value.name}`);
                } else choose(value);
            }} onMesh={(mesh) => {
                setSelectedMesh(mesh.name);
                setWeightBone(-2);
                const inspectedMesh = { ...mesh, material_name: bfres?.materials?.[mesh.material_index]?.name };
                setDetail({ type: 'mesh', value: inspectedMesh });
                setYaml(JSON.stringify(mesh, null, 2));
                setStatusText(`Selected mesh ${mesh.name}`);
            }} onBone={(bone, index) => {
                setSelectedMesh('');
                setSelected(bone);
                setDetail({ type: 'bone', value: { index, ...bone } });
                setYaml(JSON.stringify({ index, ...bone }, null, 2));
                setWeightBone(index);
                setStatusText(`Selected bone ${bone.name}`);
            }} />
            <div className="bfres-panel-divider left" role="separator" aria-orientation="vertical" onMouseDown={(event) => startPanelDrag('left', event)} />
            <section className="bfres-viewport" aria-label="BFRES 3D viewport">
                <Canvas dpr={[1, 2]} gl={{ antialias: true }} onPointerMissed={() => setSelectedMesh('')}>
                    {bfres?.render && <ResourceScene bfres={bfres} render={bfres.render} viewMode={viewMode} uvIndex={uvIndex} brightness={brightness} celShading={celShading} showSkeleton={showSkeleton} showNormals={showNormals} weightBone={weightBone} selectedMesh={selectedMesh} modelVisible={modelVisible} hiddenMeshes={hiddenMeshes} onSelectMesh={(mesh) => {
                        setSelectedMesh(mesh.name);
                        setWeightBone(-2);
                        setDetail({ type: 'mesh', value: mesh });
                        setYaml(JSON.stringify(mesh, null, 2));
                        setStatusText(`${mesh.name}: ${mesh.positions.length.toLocaleString()} vertices, ${(mesh.indices.length / 3).toLocaleString()} triangles`);
                    }} />}
                </Canvas>
                <div className="bfres-viewport-note">{bfres?.render?.meshes.length || 0} meshes · {(bfres?.render?.meshes || []).reduce((sum, mesh) => sum + mesh.positions.length, 0).toLocaleString()} vertices · {bfres?.render?.bones.length || 0} bones</div>
            </section>
            <div className="bfres-panel-divider right" role="separator" aria-orientation="vertical" onMouseDown={(event) => startPanelDrag('right', event)} />
            <aside className="bfres-inspector">
                {panel === 'resources' && <NodeInspector detail={detail} />}
                {panel === 'parameters' && bfres && <dl className="bfres-parameters">
                    <dt>Version</dt><dd>{bfres.header.version.join('.')}</dd>
                    <dt>Endian</dt><dd>{bfres.header.endian}</dd>
                    <dt>Address size</dt><dd>{bfres.header.target_address_size || 8} bytes</dd>
                    <dt>Alignment</dt><dd>2^{bfres.header.alignment_exponent}</dd>
                    <dt>Logical size</dt><dd>{bfres.header.file_size.toLocaleString()} bytes</dd>
                    <dt>String pool</dt><dd>0x{bfres.header.string_pool_offset.toString(16).toUpperCase()} · {bfres.header.string_pool_size.toLocaleString()} bytes</dd>
                    <dt>Relocation table</dt><dd>0x{bfres.header.relocation_table_offset.toString(16).toUpperCase()}</dd>
                    <dt>Sections</dt><dd>{bfres.sections.length}</dd>
                </dl>}
                {panel === 'animations' && <div className="bfres-animation-list">
                    {animations.length === 0 && <p>No animation sections in this BFRES.</p>}
                    {animations.map((animation) => <button type="button" key={animation.offset} onClick={() => choose(animation)}>
                        <span>▶</span><div><strong>{animation.name || String.fromCharCode(...animation.signature)}</strong><small>{String.fromCharCode(...animation.signature)} · playback decoding pending</small></div>
                    </button>)}
                </div>}
                {showEditor && <section className="bfres-yaml-panel">
                    <header><strong>{selected?.name || 'Node YAML'}</strong><button type="button" onClick={applyYaml} disabled={!selected}>Stage YAML</button></header>
                    <Editor height="100%" language="yaml" theme="vs-dark" value={yaml} onChange={(value) => setYaml(value || '')} options={{ minimap: { enabled: false }, fontSize: 12, automaticLayout: true, scrollBeyondLastLine: false }} />
                </section>}
            </aside>
            <ResourceContextMenu menu={contextMenu} close={() => setContextMenu(null)} action={(label, menu) => setStatusText(`${label}: ${menu.name}`)} />
        </>}
    </main>;
}

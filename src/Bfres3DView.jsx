import Editor from '@monaco-editor/react';
import { Canvas } from '@react-three/fiber';
import { Bounds, Grid, OrbitControls, PerspectiveCamera } from '@react-three/drei';
import { useEffect, useMemo, useState, useSyncExternalStore } from 'react';
import * as THREE from 'three';
import { getDocumentsSnapshot, invoke, subscribeDocuments } from './DocumentState';
import './Bfres3DView.css';

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
    const base = find('Base color') || find('Texture');
    const emission = find('Emission');
    if (base) base.colorSpace = THREE.SRGBColorSpace;
    if (emission) emission.colorSpace = THREE.SRGBColorSpace;
    return {
        base,
        normal: find('Normal'),
        parameters: find('Material parameters'),
        emission,
        mask: find('Mask'),
        specular: find('Specular'),
    };
}

function RenderMesh({ mesh, bones, scaleMode, viewMode, weightBone, showNormals, onSelect, textures }) {
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
        if (mesh.uv0.length === mesh.positions.length) result.setAttribute('uv', new THREE.BufferAttribute(new Float32Array(mesh.uv0.flat()), 2));
        const colors = new Float32Array(mesh.positions.length * 3);
        mesh.positions.forEach((_, vertex) => {
            let strength = 0;
            if (weightBone >= 0) {
                (mesh.bone_indices[vertex] || []).forEach((bone, influence) => {
                    if (bone === weightBone) strength += mesh.bone_weights[vertex]?.[influence] ?? (influence === 0 ? 1 : 0);
                });
            }
            const normal = mesh.normals[vertex] || [0, 1, 0];
            const uv = mesh.uv0[vertex] || [0, 0];
            let color = new THREE.Color('#aeb8c2');
            if (viewMode === 'selectedBoneWeights') color = new THREE.Color().setHSL((1 - Math.min(strength, 1)) * 0.66, 1, 0.5);
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
    }, [mesh, bones, scaleMode, viewMode, weightBone]);
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
                    ? <meshBasicMaterial map={textures.normal} side={THREE.DoubleSide} />
                : viewMode === 'specularMap' && textures.specular
                    ? <meshBasicMaterial map={textures.specular} side={THREE.DoubleSide} />
                : ['metalnessMap', 'roughnessMap'].includes(viewMode) && textures.parameters
                    ? <meshBasicMaterial map={textures.parameters} side={THREE.DoubleSide} />
                : viewMode === 'emissionMap' && textures.emission
                    ? <meshBasicMaterial map={textures.emission} side={THREE.DoubleSide} />
                : ['default', 'lighting', 'diffuse', 'wireframe'].includes(viewMode)
                    ? <meshPhysicalMaterial map={textures.base} normalMap={textures.normal} roughnessMap={textures.parameters} metalnessMap={textures.parameters} emissiveMap={textures.emission} emissive={textures.emission ? '#ffffff' : '#000000'} alphaMap={textures.mask} specularColorMap={textures.specular} vertexColors={!textures.base} wireframe={viewMode === 'wireframe'} roughness={viewMode === 'diffuse' ? 1 : 0.72} metalness={viewMode === 'lighting' ? 0 : 0.05} side={THREE.DoubleSide} transparent={Boolean(textures.mask || textures.base)} alphaTest={textures.mask ? 0.2 : textures.base ? 0.02 : 0} />
                    : <meshBasicMaterial vertexColors side={THREE.DoubleSide} />}
        </mesh>
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

function ResourceScene({ bfres, render, viewMode, showSkeleton, showNormals, weightBone, selectedMesh, onSelectMesh, modelVisible, hiddenMeshes }) {
    const textures = useResolvedTextures(bfres?.resolvedTextures);
    return <>
        <color attach="background" args={['#11151b']} />
        <ambientLight intensity={1.4} />
        <directionalLight position={[6, 10, 8]} intensity={2.2} />
        <PerspectiveCamera makeDefault position={[8, 6, 10]} fov={42} />
        <OrbitControls makeDefault enableDamping dampingFactor={0.08} />
        <Grid infiniteGrid fadeDistance={45} fadeStrength={4} cellColor="#33404d" sectionColor="#53687a" />
        <Bounds fit clip observe margin={1.15}>
            <group visible={modelVisible}>{render.meshes.map((mesh, index) => <RenderMesh key={`${mesh.name}-${index}`} mesh={{ ...mesh, selected: mesh.name === selectedMesh, hidden: hiddenMeshes.includes(mesh.name) }} bones={render.bones} scaleMode={render.scale_mode} viewMode={viewMode} weightBone={weightBone} showNormals={showNormals} onSelect={onSelectMesh} textures={materialTextures(bfres?.materials?.[mesh.material_index], textures)} />)}</group>
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
                <Folder label={modelName} open checked={modelVisible} onToggle={onToggleModel} onSelect={() => onModel(modelName)} onContextMenu={(event) => onContext(event, 'model', modelName)}>
                    <Folder label="Objects" open detail={meshes.length}>{meshes.map((mesh, index) => node(mesh.name, `${mesh.positions.length} vertices`, () => onMesh(mesh), `mesh-${index}`, 'object'))}</Folder>
                    <Folder label="Materials" open detail={materials.length}>{materials.map((material) => node(material.name, `${material.texture_slots.length} textures`, () => onSection(material, 'material'), `material-${material.offset}`, 'material'))}</Folder>
                    <Folder label="Skeleton" open detail={bones.length}>{boneNodes(-1)}</Folder>
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
        {tab === 'Textures' ? <><table className="bfres-texture-table"><thead><tr><th>Texture</th><th>Type</th><th>Sampler</th></tr></thead><tbody>{material.texture_slots.map((slot) => <tr key={slot.index}><td>{slot.name}</td><td>{slot.texture_type}</td><td>_{slot.texture_type.toLowerCase().replace(/\W/g, '')}{slot.index}</td></tr>)}</tbody></table><div className="bfres-action-grid"><button type="button">Add</button><button type="button">Remove</button><button type="button">Edit</button></div></> : <div className="bfres-empty-detail">No decoded {tab.toLowerCase()} entries.</div>}
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
    const [showSkeleton, setShowSkeleton] = useState(false);
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
        if (activeTab !== '3D' || !document?.fullPath) return;
        let cancelled = false;
        setError('');
        setModelVisible(true);
        setHiddenMeshes([]);
        invoke('inspect_3d_model', { path: document.fullPath }).then((value) => {
            if (cancelled) return;
            setBfres(value);
            if (value.materials) {
                const requested = new Set(value.materials.flatMap((material) => material.texture_slots.map((slot) => slot.name))).size;
                setStatusText(`Loaded ${value.resolvedTextures?.length || 0} of ${requested} referenced TexToGo textures`);
            }
            const initial = value.sections.find((section) => String.fromCharCode(...section.signature) === 'FMDL') || value.sections[0];
            setSelected(initial || null);
            setYaml(initial ? sectionYaml(initial) : '');
        }).catch((reason) => {
            if (!cancelled) setError(String(reason));
        });
        return () => { cancelled = true; };
    }, [activeTab, document?.fullPath]);

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

    if (activeTab !== '3D') return null;
    return <main className="bfres-workspace" style={{ '--bfres-left-width': `${leftWidth}px`, '--bfres-right-width': `${rightWidth}px` }}>
        <header className="bfres-viewport-toolbar">
            <button type="button" onClick={() => setPanel('resources')} className={panel === 'resources' ? 'active' : ''}>Resources</button>
            <button type="button" onClick={() => setPanel('parameters')} className={panel === 'parameters' ? 'active' : ''}>Parameters</button>
            <button type="button" onClick={() => setPanel('animations')} className={panel === 'animations' ? 'active' : ''}>Animations <small>{animations.length}</small></button>
            <label className="bfres-shading-select">Shading:
                <select value={viewMode} onChange={(event) => { setViewMode(event.target.value); if (event.target.value === 'selectedBoneWeights' && weightBone < 0) setWeightBone(0); }}>
                    <option value="default">Default</option><option value="normal">Normal</option><option value="lighting">Lighting</option><option value="diffuse">Diffuse</option><option value="normalMap">NormalMap</option><option value="vertColor">VertColor</option><option value="ambientOcclusion">AmbientOcclusion</option><option value="uvCoords">UVCoords</option><option value="uvTestPattern">UVTestPattern</option><option value="tangents">Tangents</option><option value="bitangents">Bitangents</option><option value="lightMap">LightMap</option><option value="selectedBoneWeights">SelectedBoneWeights</option><option value="specularMap">SpecularMap</option><option value="shadowMap">ShadowMap</option><option value="metalnessMap">MetalnessMap</option><option value="roughnessMap">RoughnessMap</option><option value="subSurfaceScatteringMap">SubSurfaceScatteringMap</option><option value="emissionMap">EmissionMap</option><option value="wireframe">Wireframe</option>
                </select>
            </label>
            <button type="button" onClick={() => setShowSkeleton((value) => !value)} className={showSkeleton ? 'active' : ''}>Skeleton</button>
            <button type="button" onClick={() => setShowNormals((value) => !value)} className={showNormals ? 'active' : ''}>Normals</button>
            <button type="button" onClick={() => setShowEditor((value) => !value)} className={!showEditor ? 'active' : ''}>{showEditor ? 'Hide YAML' : 'Show YAML'}</button>
            {viewMode === 'selectedBoneWeights' && <select className="bfres-bone-select" value={weightBone} onChange={(event) => setWeightBone(Number(event.target.value))} aria-label="Selected bone weights">
                {(bfres?.render?.bones || []).map((bone, index) => <option key={`${bone.name}-${index}`} value={index}>Bone: {bone.name}</option>)}
            </select>}
        </header>
        {error ? <div className="bfres-error">{error}</div> : <>
            <ResourceTree bfres={bfres} title={document?.title} modelVisible={modelVisible} hiddenMeshes={hiddenMeshes} onToggleModel={() => setModelVisible((value) => !value)} onToggleMesh={(name) => setHiddenMeshes((values) => values.includes(name) ? values.filter((value) => value !== name) : [...values, name])} onContext={(event, kind, name) => { event.preventDefault(); setContextMenu({ x: event.clientX, y: event.clientY, kind, name }); }} onModel={(name) => {
                setSelectedMesh('');
                const model = { name, path: document?.fullPath || '', vertex_buffer_count: bfres?.render?.meshes.length || 0, shape_count: bfres?.render?.meshes.length || 0, material_count: bfres?.materials?.length || 0, user_data_count: 0, total_vertex_count: (bfres?.render?.meshes || []).reduce((sum, mesh) => sum + mesh.positions.length, 0) };
                setDetail({ type: 'model', value: model });
                setYaml(JSON.stringify(model, null, 2));
                setStatusText(`Selected model ${name}`);
            }} onSection={(value, type) => {
                setSelectedMesh('');
                if (type === 'material' || type === 'texture') {
                    setSelected(value);
                    setDetail({ type, value });
                    setYaml(JSON.stringify(value, null, 2));
                    setStatusText(`Selected ${type} ${value.name}`);
                } else choose(value);
            }} onMesh={(mesh) => {
                setSelectedMesh(mesh.name);
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
                    {bfres?.render && <ResourceScene bfres={bfres} render={bfres.render} viewMode={viewMode} showSkeleton={showSkeleton} showNormals={showNormals} weightBone={weightBone} selectedMesh={selectedMesh} modelVisible={modelVisible} hiddenMeshes={hiddenMeshes} onSelectMesh={(mesh) => {
                        setSelectedMesh(mesh.name);
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

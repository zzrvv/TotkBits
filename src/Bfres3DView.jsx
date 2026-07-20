import Editor from '@monaco-editor/react';
import { Canvas } from '@react-three/fiber';
import { Bounds, Grid, OrbitControls, PerspectiveCamera } from '@react-three/drei';
import { invoke } from '@tauri-apps/api/core';
import { useEffect, useMemo, useState, useSyncExternalStore } from 'react';
import * as THREE from 'three';
import { getDocumentsSnapshot, subscribeDocuments } from './DocumentState';
import './Bfres3DView.css';

const COLORS = {
    FMDL: '#5ac8fa', FSKL: '#ffd166', FVTX: '#74c69d', FSHP: '#48cae4',
    FMAT: '#f28482', FSKA: '#c77dff', FSHU: '#ff9f1c', FSHA: '#e76f51',
    FSCN: '#90be6d', FTXP: '#f9c74f', FVIS: '#adb5bd', FMAA: '#f72585', FREL: '#6c757d',
};

const sectionYaml = (section) => [
    `type: ${section.signature.join ? String.fromCharCode(...section.signature) : section.signature}`,
    `name: ${section.name ?? 'null'}`,
    `offset: 0x${Number(section.offset).toString(16).toUpperCase()}`,
    'parameters: {}',
].join('\n');

function boneWorldMatrices(bones) {
    const matrices = bones.map(() => new THREE.Matrix4());
    bones.forEach((bone, index) => {
        const local = new THREE.Matrix4().compose(
            new THREE.Vector3(...bone.translation),
            new THREE.Quaternion(...bone.rotation).normalize(),
            new THREE.Vector3(...bone.scale),
        );
        matrices[index] = bone.parent_index >= 0 && matrices[bone.parent_index]
            ? matrices[bone.parent_index].clone().multiply(local) : local;
    });
    return matrices;
}

function RenderMesh({ mesh, bones, wireframe, weightBone, showNormals, onSelect }) {
    const geometry = useMemo(() => {
        const result = new THREE.BufferGeometry();
        const positions = new Float32Array(mesh.positions.flat());
        const normals = mesh.normals.length === mesh.positions.length ? new Float32Array(mesh.normals.flat()) : null;
        if (mesh.vertex_skin_count <= 1 && bones.length) {
            const worlds = boneWorldMatrices(bones);
            const fallback = mesh.bone_index;
            for (let index = 0; index < mesh.positions.length; index += 1) {
                const bone = mesh.vertex_skin_count === 1 ? mesh.bone_indices[index]?.[0] : fallback;
                const matrix = worlds[bone];
                if (!matrix) continue;
                const position = new THREE.Vector3(positions[index * 3], positions[index * 3 + 1], positions[index * 3 + 2]).applyMatrix4(matrix);
                positions.set(position.toArray(), index * 3);
                if (normals) {
                    const normalMatrix = new THREE.Matrix3().getNormalMatrix(matrix);
                    const normal = new THREE.Vector3(normals[index * 3], normals[index * 3 + 1], normals[index * 3 + 2]).applyMatrix3(normalMatrix).normalize();
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
            const color = weightBone >= 0
                ? new THREE.Color().setHSL((1 - Math.min(strength, 1)) * 0.66, 1, 0.5)
                : weightBone === -1 && mesh.colors[vertex] ? new THREE.Color(mesh.colors[vertex][0], mesh.colors[vertex][1], mesh.colors[vertex][2]) : new THREE.Color('#aeb8c2');
            colors.set(color.toArray(), vertex * 3);
        });
        result.setAttribute('color', new THREE.BufferAttribute(colors, 3));
        result.setIndex(mesh.indices);
        result.computeBoundingSphere();
        return result;
    }, [mesh, bones, weightBone]);
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
    return <group>
        <mesh geometry={geometry} onClick={(event) => { event.stopPropagation(); onSelect(mesh); }} castShadow receiveShadow>
            <meshStandardMaterial vertexColors wireframe={wireframe} roughness={0.72} metalness={0.05} side={THREE.DoubleSide} />
        </mesh>
        {showNormals && <lineSegments geometry={normalLines}><lineBasicMaterial color="#55e6ff" depthTest={false} transparent opacity={0.8} /></lineSegments>}
    </group>;
}

function Skeleton({ bones }) {
    const points = useMemo(() => {
        const worlds = boneWorldMatrices(bones);
        const values = [];
        bones.forEach((bone, index) => {
            if (bone.parent_index < 0 || !worlds[bone.parent_index]) return;
            values.push(...new THREE.Vector3().setFromMatrixPosition(worlds[bone.parent_index]).toArray());
            values.push(...new THREE.Vector3().setFromMatrixPosition(worlds[index]).toArray());
        });
        return new Float32Array(values);
    }, [bones]);
    return <lineSegments><bufferGeometry><bufferAttribute attach="attributes-position" args={[points, 3]} /></bufferGeometry><lineBasicMaterial color="#ffd166" depthTest={false} /></lineSegments>;
}

function ResourceScene({ render, wireframe, showSkeleton, showNormals, weightBone, onSelectMesh }) {
    return <>
        <color attach="background" args={['#11151b']} />
        <ambientLight intensity={1.4} />
        <directionalLight position={[6, 10, 8]} intensity={2.2} />
        <PerspectiveCamera makeDefault position={[8, 6, 10]} fov={42} />
        <OrbitControls makeDefault enableDamping dampingFactor={0.08} />
        <Grid infiniteGrid fadeDistance={45} fadeStrength={4} cellColor="#33404d" sectionColor="#53687a" />
        <Bounds fit clip observe margin={1.15}>
            <group>{render.meshes.map((mesh, index) => <RenderMesh key={`${mesh.name}-${index}`} mesh={mesh} bones={render.bones} wireframe={wireframe} weightBone={weightBone} showNormals={showNormals} onSelect={onSelectMesh} />)}</group>
            {showSkeleton && <Skeleton bones={render.bones} />}
        </Bounds>
    </>;
}

function TreeFolder({ label, count, children }) {
    return <details open className="bfres-tree-folder">
        <summary><span>▾</span>{label}<small>{count}</small></summary>
        <div>{children}</div>
    </details>;
}

function ResourceTree({ bfres, onSection, onMesh, onBone }) {
    const sections = bfres?.sections || [];
    const materials = bfres?.materials || [];
    const textures = sections.filter((section) => ['FTXP', 'FTEX', 'BNTX'].includes(String.fromCharCode(...section.signature)));
    const meshes = bfres?.render?.meshes || [];
    const bones = bfres?.render?.bones || [];
    const node = (name, detail, action, key) => <button type="button" className="bfres-tree-node" onClick={action} key={key} title={name}>
        <span>{name || 'Unnamed'}</span><small>{detail}</small>
    </button>;
    const boneNodes = (parentIndex) => bones.map((bone, index) => ({ bone, index })).filter(({ bone }) => bone.parent_index === parentIndex).map(({ bone, index }) => {
        const children = boneNodes(index);
        if (children.length === 0) return node(bone.name, '', () => onBone(bone, index), `bone-${index}`);
        return <details className="bfres-bone-branch" key={`bone-${index}`}>
            <summary onClick={() => onBone(bone, index)}><span>▸</span><strong>{bone.name}</strong><small>{children.length}</small></summary>
            <div>{children}</div>
        </details>;
    });
    return <nav className="bfres-resource-tree" aria-label="BFRES resources">
        <header>Scene collection</header>
        <TreeFolder label="Meshes" count={meshes.length}>{meshes.map((mesh, index) => node(mesh.name, `${mesh.positions.length} vertices`, () => onMesh(mesh), `mesh-${index}`))}</TreeFolder>
        <TreeFolder label="Materials" count={materials.length}>{materials.map((material) => node(material.name, `${material.texture_slots.length} textures`, () => onSection(material, 'material'), `material-${material.offset}`))}</TreeFolder>
        
        <TreeFolder label="Textures" count={textures.length}>{textures.map((section) => node(section.name, 'Unparsed texture data', () => onSection(section), `texture-${section.offset}`))}</TreeFolder>
        <TreeFolder label="Skeleton" count={bones.length}>{boneNodes(-1)}</TreeFolder>
    </nav>;
}

export default function Bfres3DView({ activeTab, setStatusText }) {
    const { documents, activeDocumentId } = useSyncExternalStore(subscribeDocuments, getDocumentsSnapshot);
    const document = documents.find((item) => item.id === activeDocumentId);
    const [bfres, setBfres] = useState(null);
    const [error, setError] = useState('');
    const [filter, setFilter] = useState('');
    const [selected, setSelected] = useState(null);
    const [yaml, setYaml] = useState('');
    const [panel, setPanel] = useState('resources');
    const [wireframe, setWireframe] = useState(false);
    const [showSkeleton, setShowSkeleton] = useState(false);
    const [showNormals, setShowNormals] = useState(false);
    const [weightBone, setWeightBone] = useState(-2);
    const [detail, setDetail] = useState(null);
    const [showEditor, setShowEditor] = useState(false);

    useEffect(() => {
        if (activeTab !== '3D' || !document?.fullPath) return;
        let cancelled = false;
        setError('');
        invoke('inspect_bfres', { path: document.fullPath }).then((value) => {
            if (cancelled) return;
            setBfres(value);
            const initial = value.sections.find((section) => String.fromCharCode(...section.signature) === 'FMDL') || value.sections[0];
            setSelected(initial || null);
            setYaml(initial ? sectionYaml(initial) : '');
        }).catch((reason) => {
            if (!cancelled) setError(String(reason));
        });
        return () => { cancelled = true; };
    }, [activeTab, document?.fullPath]);

    const sections = useMemo(() => {
        const query = filter.trim().toLowerCase();
        if (!query) return bfres?.sections || [];
        return (bfres?.sections || []).filter((section) => {
            const signature = String.fromCharCode(...section.signature);
            return signature.toLowerCase().includes(query) || (section.name || '').toLowerCase().includes(query);
        });
    }, [bfres, filter]);
    const animations = useMemo(() => (bfres?.sections || []).filter((section) =>
        ['FSKA', 'FSHU', 'FSHA', 'FTXP', 'FVIS', 'FMAA'].includes(String.fromCharCode(...section.signature))), [bfres]);

    const choose = (section) => {
        setSelected(section);
        setDetail(null);
        setYaml(sectionYaml(section));
    };
    const applyYaml = () => {
        setStatusText('BFRES node YAML is staged in the inspector; binary rebuilding is not available for this node type yet');
    };

    if (activeTab !== '3D') return null;
    return <main className="bfres-workspace">
        <header className="bfres-toolbar">
            <div><strong>{document?.title || 'BFRES'}</strong><span>{bfres?.name || 'Resource file'}</span></div>
            <button type="button" onClick={() => setPanel('resources')} className={panel === 'resources' ? 'active' : ''}>Resources</button>
            <button type="button" onClick={() => setPanel('parameters')} className={panel === 'parameters' ? 'active' : ''}>Parameters</button>
            <button type="button" onClick={() => setPanel('animations')} className={panel === 'animations' ? 'active' : ''}>Animations <small>{animations.length}</small></button>
            <button type="button" onClick={() => setWireframe((value) => !value)} className={wireframe ? 'active' : ''}>Wireframe</button>
            <button type="button" onClick={() => setShowSkeleton((value) => !value)} className={showSkeleton ? 'active' : ''}>Skeleton</button>
            <button type="button" onClick={() => setShowNormals((value) => !value)} className={showNormals ? 'active' : ''}>Normals</button>
            <button type="button" onClick={() => setShowEditor((value) => !value)} className={!showEditor ? 'active' : ''}>{showEditor ? 'Hide YAML' : 'Show YAML'}</button>
            <select value={weightBone} onChange={(event) => setWeightBone(Number(event.target.value))} aria-label="Visualize vertex weights">
                <option value={-2}>Solid</option>
                <option value={-1}>Vertex colors</option>
                {(bfres?.render?.bones || []).map((bone, index) => <option key={`${bone.name}-${index}`} value={index}>Weights: {bone.name}</option>)}
            </select>
        </header>
        {error ? <div className="bfres-error">{error}</div> : <>
            <ResourceTree bfres={bfres} onSection={(value, type) => {
                if (type === 'material') {
                    setSelected(value);
                    setDetail({ type, value });
                    setYaml(JSON.stringify(value, null, 2));
                    setStatusText(`Selected material ${value.name}`);
                } else choose(value);
            }} onMesh={(mesh) => {
                setDetail({ type: 'mesh', value: mesh });
                setYaml(JSON.stringify(mesh, null, 2));
                setStatusText(`Selected mesh ${mesh.name}`);
            }} onBone={(bone, index) => {
                setSelected(bone);
                setDetail({ type: 'bone', value: { index, ...bone } });
                setYaml(JSON.stringify({ index, ...bone }, null, 2));
                setWeightBone(index);
                setStatusText(`Selected bone ${bone.name}`);
            }} />
            <section className="bfres-viewport" aria-label="BFRES 3D viewport">
                <Canvas dpr={[1, 2]} gl={{ antialias: true }}>
                    {bfres?.render && <ResourceScene render={bfres.render} wireframe={wireframe} showSkeleton={showSkeleton} showNormals={showNormals} weightBone={weightBone} onSelectMesh={(mesh) => {
                        setYaml(JSON.stringify(mesh, null, 2));
                        setStatusText(`${mesh.name}: ${mesh.positions.length.toLocaleString()} vertices, ${(mesh.indices.length / 3).toLocaleString()} triangles`);
                    }} />}
                </Canvas>
                <div className="bfres-viewport-note">{bfres?.render?.meshes.length || 0} meshes · {(bfres?.render?.meshes || []).reduce((sum, mesh) => sum + mesh.positions.length, 0).toLocaleString()} vertices · {bfres?.render?.bones.length || 0} bones</div>
            </section>
            <aside className="bfres-inspector">
                {detail?.type === 'material' && <section className="bfres-node-detail">
                    <header><strong>{detail.value.name}</strong><small>BFMAT material</small></header>
                    <h4>Texture slots</h4>
                    {detail.value.texture_slots.length === 0 ? <p>No texture references.</p> : <div className="bfres-slot-list">
                        {detail.value.texture_slots.map((slot) => <div key={slot.index}><span>{slot.index}</span><strong>{slot.name}</strong><small>{slot.texture_type}</small></div>)}
                    </div>}
                </section>}
                {detail?.type === 'bone' && <section className="bfres-node-detail">
                    <header><strong>{detail.value.name}</strong><small>Bone #{detail.value.index}</small></header>
                    <dl><dt>Parent</dt><dd>{detail.value.parent_index}</dd><dt>Scale</dt><dd>{detail.value.scale.join(', ')}</dd><dt>Rotation</dt><dd>{detail.value.rotation.join(', ')}</dd><dt>Translation</dt><dd>{detail.value.translation.join(', ')}</dd><dt>Smooth matrix</dt><dd>{detail.value.smooth_matrix_index}</dd><dt>Rigid matrix</dt><dd>{detail.value.rigid_matrix_index}</dd></dl>
                </section>}
                {panel === 'resources' && <>
                    <input value={filter} onChange={(event) => setFilter(event.target.value)} placeholder="Filter name or type…" aria-label="Filter BFRES resources" />
                    <div className="bfres-resource-table" role="table">
                        <div className="bfres-row bfres-table-head" role="row"><span>Type</span><span>Name</span><span>Offset</span></div>
                        {sections.map((section) => {
                            const signature = String.fromCharCode(...section.signature);
                            return <button type="button" className={`bfres-row ${selected?.offset === section.offset ? 'selected' : ''}`} key={`${signature}-${section.offset}`} onClick={() => choose(section)}>
                                <span style={{ color: COLORS[signature] }}>{signature}</span><span>{section.name || 'Unnamed'}</span><span>0x{Number(section.offset).toString(16).toUpperCase()}</span>
                            </button>;
                        })}
                    </div>
                </>}
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
        </>}
    </main>;
}

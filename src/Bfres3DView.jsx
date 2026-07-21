import Editor from '@monaco-editor/react';
import { Canvas } from '@react-three/fiber';
import { Bounds, Grid, OrbitControls, PerspectiveCamera } from '@react-three/drei';
import { invoke } from '@tauri-apps/api/core';
import { useEffect, useMemo, useState, useSyncExternalStore } from 'react';
import * as THREE from 'three';
import { getDocumentsSnapshot, subscribeDocuments } from './DocumentState';
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

function RenderMesh({ mesh, bones, scaleMode, wireframe, weightBone, showNormals, onSelect }) {
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
            const color = weightBone >= 0
                ? new THREE.Color().setHSL((1 - Math.min(strength, 1)) * 0.66, 1, 0.5)
                : weightBone === -1 && mesh.colors[vertex] ? new THREE.Color(mesh.colors[vertex][0], mesh.colors[vertex][1], mesh.colors[vertex][2]) : new THREE.Color('#aeb8c2');
            colors.set(color.toArray(), vertex * 3);
        });
        result.setAttribute('color', new THREE.BufferAttribute(colors, 3));
        result.setIndex(mesh.indices);
        result.computeBoundingSphere();
        return result;
    }, [mesh, bones, scaleMode, weightBone]);
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
        <mesh geometry={geometry} onClick={(event) => { event.stopPropagation(); onSelect(mesh); }} castShadow receiveShadow>
            <meshStandardMaterial vertexColors wireframe={wireframe} roughness={0.72} metalness={0.05} side={THREE.DoubleSide} />
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

function ResourceScene({ render, wireframe, showSkeleton, showNormals, weightBone, selectedMesh, onSelectMesh }) {
    return <>
        <color attach="background" args={['#11151b']} />
        <ambientLight intensity={1.4} />
        <directionalLight position={[6, 10, 8]} intensity={2.2} />
        <PerspectiveCamera makeDefault position={[8, 6, 10]} fov={42} />
        <OrbitControls makeDefault enableDamping dampingFactor={0.08} />
        <Grid infiniteGrid fadeDistance={45} fadeStrength={4} cellColor="#33404d" sectionColor="#53687a" />
        <Bounds fit clip observe margin={1.15}>
            <group>{render.meshes.map((mesh, index) => <RenderMesh key={`${mesh.name}-${index}`} mesh={{ ...mesh, selected: mesh.name === selectedMesh }} bones={render.bones} scaleMode={render.scale_mode} wireframe={wireframe} weightBone={weightBone} showNormals={showNormals} onSelect={onSelectMesh} />)}</group>
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

function Folder({ label, children, open = false, detail }) {
    return <details open={open} className="bfres-folder-node">
        <summary><span className="bfres-folder-arrow">›</span><span className="bfres-folder-icon">■</span><strong>{label}</strong>{detail && <small>{detail}</small>}</summary>
        <div>{children}</div>
    </details>;
}

function ResourceTree({ bfres, title, onSection, onMesh, onBone }) {
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
    const modelName = sections.find((section) => String.fromCharCode(...section.signature) === 'FMDL')?.name || bfres?.name || 'Model';
    return <nav className="bfres-resource-tree" aria-label="BFRES resources">
        <div className="bfres-tree-actions"><button type="button" title="Expand resources">＋</button><span>Resources</span></div>
        <Folder label={title || bfres?.name || 'BFRES'} open>
            <Folder label="Models" open detail="1">
                <Folder label={modelName} open>
                    <Folder label="Objects" open detail={meshes.length}>{meshes.map((mesh, index) => node(mesh.name, `${mesh.positions.length} vertices`, () => onMesh(mesh), `mesh-${index}`))}</Folder>
                    <Folder label="Materials" open detail={materials.length}>{materials.map((material) => node(material.name, `${material.texture_slots.length} textures`, () => onSection(material, 'material'), `material-${material.offset}`))}</Folder>
                    <Folder label="Skeleton" open detail={bones.length}>{boneNodes(-1)}</Folder>
                </Folder>
            </Folder>
            <Folder label="Textures" detail={textures.length}>{textures.map((section) => node(section.name, 'Texture', () => onSection(section), `texture-${section.offset}`))}</Folder>
            <Folder label="Animations" detail={(bfres?.sections || []).filter((section) => ['FSKA', 'FSHU', 'FSHA', 'FVIS', 'FMAA'].includes(String.fromCharCode(...section.signature))).length} />
            <Folder label="Embedded Files" />
            <Folder label="TexToGo" />
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
    return <section className="bfres-selected-detail">
        <header><strong>{detail.value.name || 'Unnamed'}</strong><small>{detail.type}</small></header>
        <PropertyList value={detail.value} />
    </section>;
}

export default function Bfres3DView({ activeTab, setStatusText }) {
    const { documents, activeDocumentId } = useSyncExternalStore(subscribeDocuments, getDocumentsSnapshot);
    const document = documents.find((item) => item.id === activeDocumentId);
    const [bfres, setBfres] = useState(null);
    const [error, setError] = useState('');
    const [selected, setSelected] = useState(null);
    const [yaml, setYaml] = useState('');
    const [panel, setPanel] = useState('resources');
    const [wireframe, setWireframe] = useState(false);
    const [showSkeleton, setShowSkeleton] = useState(false);
    const [showNormals, setShowNormals] = useState(false);
    const [weightBone, setWeightBone] = useState(-2);
    const [detail, setDetail] = useState(null);
    const [showEditor, setShowEditor] = useState(false);
    const [selectedMesh, setSelectedMesh] = useState('');
    const [leftWidth, setLeftWidth] = useState(240);
    const [rightWidth, setRightWidth] = useState(390);

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

    const animations = useMemo(() => (bfres?.sections || []).filter((section) =>
        ['FSKA', 'FSHU', 'FSHA', 'FTXP', 'FVIS', 'FMAA'].includes(String.fromCharCode(...section.signature))), [bfres]);

    const choose = (section) => {
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
            <ResourceTree bfres={bfres} title={document?.title} onSection={(value, type) => {
                if (type === 'material') {
                    setSelected(value);
                    setDetail({ type, value });
                    setYaml(JSON.stringify(value, null, 2));
                    setStatusText(`Selected material ${value.name}`);
                } else choose(value);
            }} onMesh={(mesh) => {
                setSelectedMesh(mesh.name);
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
            <div className="bfres-panel-divider left" role="separator" aria-orientation="vertical" onMouseDown={(event) => startPanelDrag('left', event)} />
            <section className="bfres-viewport" aria-label="BFRES 3D viewport">
                <Canvas dpr={[1, 2]} gl={{ antialias: true }}>
                    {bfres?.render && <ResourceScene render={bfres.render} wireframe={wireframe} showSkeleton={showSkeleton} showNormals={showNormals} weightBone={weightBone} selectedMesh={selectedMesh} onSelectMesh={(mesh) => {
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
        </>}
    </main>;
}

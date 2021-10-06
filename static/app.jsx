const GREY_URI = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAQAAABeK7cBAAAADUlEQVR42mM88Z+BAQAGJwHJ3qipmgAAAABJRU5ErkJggg==";

let Process = ({onKill, children: {pid, info, state, frame}}) =>
<li key={pid} className={state + " process"}>
    <div>
        <div className="info">{
            state === "error" ? "Error: " + info :
                state === "ready" ? "Idle" :
                state === "working" ? "Rendering Frame " + frame :
                state === "pending" ? "Spawning..." :
                false
        }</div>
        {["error", "ready"].includes(state) &&
         <button type="button" onClick={onKill}>×</button>
        }
    </div>
</li>;

let Processes = ({onAdd, onKill, children: processes}) =>
<section>
    <h2>Processes</h2>
    <ul className="processes">
        <li className="button">
            <div>
                <button type="button" onClick={onAdd}>+</button>
            </div>
        </li>
        {Object.entries(processes).map(([pid, process]) =>
            <Process key={pid} onKill={() => onKill(pid)}>
                {process}
            </Process>
        )}
    </ul>
</section>;

let Parameters = ({onRender, onChange, ready, children: parameters}) =>
<section>
    <h2>Parameters</h2>

    {Array.from(parameters, ([name, {label, value}]) => {
        let type = {
            "number": "number",
            "string": "text",
        }[typeof value];

        return type === undefined
            ? <div key={name}>
                  undefined parameter type {typeof value}
              </div>
            : <div key={name} className="input-group">
                  <span className="input-group-text">{label}:</span>
                  <input
                      className="form-control"
                      type={type}
                      name={name}
                      value={"" + value}
                      onChange={onChange}
                  />
              </div>;
    })}

    <div className="btn-group" role="group">
        <button
            type="button"
            className="btn btn-primary"
            onClick={onRender}
        >Render</button>
        <button
            type="button"
            className="btn btn-secondary"
            disabled={!ready}
        >Download</button>
    </div>
</section>;

let Output = ({animation = GREY_URI, children}) =>
<section className="output order-1 order-lg-2 col-lg-8">
    <h2>Output</h2>
    <img className="animation" src={animation} />
    <div className="frames">
        {children.map((frame, index) =>
            <img key={index} className={"frame frame" + index} src={frame.src || GREY_URI} />
        )}
    </div>
</section>

function blobToDataUri(blob, callback) {
    var a = new FileReader();
    a.onload = e => callback(e.target.result);
    a.readAsDataURL(blob);
}

class App extends React.Component {
    constructor(props) {
        super(props);
        this.socket = props.children;
        this.socket.onmessage = this.onMessage;
        this.awaiting = null;
        this.state = {
            animation: GREY_URI,
            frames: Array(100).fill().map(() => ({})),
            parameters: new Map([
                [ "height", { label: "Height (pixels)", value: 180 } ],
                [ "width", { label: "Width (pixels)", value: 320 } ],
                [ "total_frames", { label: "Total frames", value: 40 } ],
                [ "samples_per_pixel", { label: "Samples per pixel", value: 32 } ],
            ]),
            processes: {},
            ready: false,
        };
    }

    render = () =>
    <main className="row">
        <Output animation={this.state.animation}>{this.state.frames}</Output>
        <section className="input order-2 order-lg-1 col-lg-4">
            <Parameters
                onRender={this.onRender}
                onChange={this.onParameterChange}
                ready={this.state.ready}
            >{this.state.parameters}</Parameters>
            <Processes onAdd={this.onAdd} onKill={this.onKill}>
                {this.state.processes}
            </Processes>
        </section>
    </main>;

    onMessage = (msg) => {
        if (typeof msg.data === "string") {
            let message = JSON.parse(msg.data);
            if (message.hasOwnProperty("job"))
                this.setState({
                    parameters: new Map(message.job_fields.map(([name, type]) => [name, {
                        // we could do with a proper `label` field here
                        label: name,
                        value: message.job[name]
                            || {"integer": 0, "float": 0, "string": ""}[type],
                    }])),
                    frames: Array(message.job.total_frames).fill().map(() => ({})),
                });
            else if (message.hasOwnProperty("processes"))
                this.setState({ processes: message.processes });
            else if (message.hasOwnProperty("frame"))
                this.awaiting = message.frame;
            else if (message.hasOwnProperty("gif"))
                this.awaiting = "gif";
        } else if (msg.data instanceof Blob) {
            let awaiting = this.awaiting;
            this.awaiting = null;
            if (awaiting === null)
                console.log("not expecting a binary message");
            else blobToDataUri(msg.data, uri =>
                awaiting === "gif" ? this.setState({ animation: uri, ready: true })
                    : typeof awaiting === "number" ? this.updateFrame(awaiting, { src: uri })
                    : console.log("unexpected value for `awaiting`", awaiting));
        } else console.log("unexpected message type", msg);
    };

    updateFrame = (id, frame) => {
        this.setState(s => ({
            frames: s.frames.map(
                (frame_, id_) => id === id_
                    ? { ...frame_, ...frame }
                    : frame_
            ),
        }));
    };

    updateProcess = (id, process) => {
        this.setState(s => ({
            processes: {
                ...s.processes,
                [id]: {...(s.processes[id] || {}), ...process },
            },
        }));
    };

    updateParameter = (name, value) => {
        this.setState(s => ({
            parameters: new Map([
                ...s.parameters.entries(),
                [name, { ...s.parameters.get(name), value }],
            ]),
        }));
    };

    onRender = () => {
        this.setState(s => ({
            animation: GREY_URI,
            frames: Array(s.parameters.get("total_frames").value).fill().map(() => ({})),
            ready: false,
        }), () => {
            this.socket.send(JSON.stringify(Object.fromEntries(
                Array.from(this.state.parameters.entries(), ([k, v]) => [k, v.value])
            )));
        });
    };

    onAdd = () => {
        this.socket.send(JSON.stringify({ add_process: null }));
    };

    onParameterChange = (event) => {
        let value = event.target.type === "number"
            ? parseFloat(event.target.value)
            : event.target.value;
        this.updateParameter(event.target.name, value);
    };

    onKill = (pid) => {
        this.socket.send(JSON.stringify({ kill_process: pid }));
        this.setState({ ...this.state.processes, [pid]: undefined });
    };
}

ReactDOM.render(<App>{
    location.protocol === "file:"
        ? new MockSocket()
        : new WebSocket("ws://" + location.host + "/ws")
}</App>, document.getElementById("app"));

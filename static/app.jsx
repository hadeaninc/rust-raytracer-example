const GREY = "data:image/png;base64, iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAQAAABeK7cBAAAADUlEQVR42mM88Z+BAQAGJwHJ3qipmgAAAABJRU5ErkJggg==";
const MAGENTA = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAAEElEQVR42mP8z/D/PwMQAAAS/wL/eBxg8AAAAABJRU5ErkJggg==";

let Process = ({onKill, children: {pid, info, state}}) =>
<li key={pid} className={state + " process"}>
    <div>
        <div className="info">{info}</div>
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
        {Array.from(processes, ([pid, process]) => {
            return <Process key={pid} onKill={() => onKill(pid)}>
                {process}
            </Process>
        })}
    </ul>
</section>;

let Parameters = ({onRender, ready, children: parameters}) =>
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
                      value={value}
                  />
              </div>;
    })}

    <div className="btn-group" role="group">
        <button
            type="button"
            className="btn btn-primary"
            onClick={() => onRender(parameters)}
        >Render</button>
        <button
            type="button"
            className="btn btn-secondary"
            disabled={!ready}
        >Download</button>
    </div>
</section>;

let Output = ({gif = GREY, children}) =>
<section className="output order-1 order-lg-2 col-lg-8">
    <h2>Output</h2>
    <img className="animation" src={gif} />
    <div className="frames">
        {children.map((frame, index) =>
            <img key={index} className="frame" src={frame.src || GREY} />
        )}
    </div>
</section>

class App extends React.Component {
    constructor(props) {
        super(props);
        this.processes = new Map();
        this.state = {
            animation: GREY,
            frames: [{}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}],
            parameters: new Map([
                [ "height", { label: "Height (pixels)", value: 180 } ],
                [ "width", { label: "Width (pixels)", value: 320 } ],
                [ "total_frames", { label: "Total frames", value: 40 } ],
                [ "samples_per_pixel", { label: "Samples per pixel", value: 32 } ],
            ]),
            processes: new Map([
                [ "1", { state: "working", info: "Rendering Frame 10", frame: 10 } ],
                [ "2", { state: "ready", info: "Idle" } ],
                [ "3", { state: "error", info: "Spawn Resources Unavailable" } ],
            ]),
        };
    }

    render = () =>
    <main className="row">
        <Output>{this.state.frames}</Output>
        <section className="input order-2 order-lg-1 col-lg-4">
            <Parameters onRender={this.onRender}>
                {this.state.parameters}
            </Parameters>
            <Processes onAdd={this.onAdd} onKill={this.onKill}>
                {this.state.processes}
            </Processes>
        </section>
    </main>;

    updateFrame = (id, frame) => {
        this.setState({
            frames: this.state.frames.map(
                (frame_, id_) => id == id_
                    ? { ...frame_, ...frame }
                    : frame,
            ),
        });
    };

    updateProcess = (id, process) => {
        this.setState({
            processes: new Map(Array.from(
                this.state.processes,
                ([id_, process_]) => id !== id_
                    ? [id_, process_]
                    : [id_, { ...process_, ...process }],
            )),
        });
    };

    onRender = (parameters) => {
        console.log("todo");
    };

    onAdd = () => {
        let pid = Math.max(0, ...this.state.processes.keys()) + 1;

        this.setState({
            processes: new Map([
                [pid, { state: "pending", info: "Spawning..." }],
                ...this.state.processes,
            ]),
        });

        setTimeout(() => {
            this.updateProcess(pid, { state: "ready", info: "Idle" });
        }, 2000);
    }

    onKill = (pid) => {
        let processes = new Map(this.state.processes);
        processes.delete(pid);
        this.setState({ processes });
    }
}

ReactDOM.render(<App />, document.getElementById("app"));

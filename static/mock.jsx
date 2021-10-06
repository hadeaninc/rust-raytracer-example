const PINK_STR = atob("iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAAEElEQVR42mP8z/D/PwMQAAAS/wL/eBxg8AAAAABJRU5ErkJggg==");
const PINK_BLOB = new Blob(
    [new Uint8Array(Array(PINK_STR.length).fill().map((_, i) => PINK_STR.charCodeAt(i)))],
    {type: "image/png"},
);

function processSpawnTime(_pid) {
    return 500 + Math.floor(Math.random() * 1000)
}

function frameRenderTime(_frame) {
    return 500 + Math.floor(Math.random() * 1000)
}

class MockSocket {
    constructor() {
        this.job = null;
        this.processes = {};
        this.remainingFrames = 0;
        this.nextFrame = 0;
        this.nextPid = 0;
    }

    runProcess = (pid) => {
        let process = this.processes[pid];
        if (!process || !this.job) return;

        if (process.state === "working") {
            this.onmessage({ data: JSON.stringify({ frame: process.frame }) });
            this.onmessage({ data: PINK_BLOB });
            process.state = "ready";
            if (!--this.remainingFrames) {
                this.onmessage({ data: JSON.stringify({ gif: null })});
                this.onmessage({ data: PINK_BLOB });
            }
        }

        if (process.state === "ready" && this.nextFrame < this.job.total_frames) {
            process.state = "working";
            process.frame = this.nextFrame++;
            setTimeout(() => this.runProcess(pid), frameRenderTime(process.frame));
        }

        this.updateProcesses();
    };

    runJob(job) {
        this.job = job;
        this.nextFrame = 0;
        this.remainingFrames = job.total_frames;

        for (let [pid, process] of Object.entries(this.processes)) {
            process.state = "ready";
            this.runProcess(pid);
        }
    }

    updateProcesses = () => {
        this.onmessage_({ data: JSON.stringify({ processes: this.processes }) });
    }

    send(msg) {
        let message = JSON.parse(msg);
        if (message.hasOwnProperty("add_process")) {
            let pid = ++this.nextPid;
            this.processes[pid] = { state: "pending" };
            this.updateProcesses();
            setTimeout(() => {
                this.processes[pid].state = "ready";
                this.updateProcesses();
                this.runProcess(pid);
            }, processSpawnTime(pid));
        } else if (message.hasOwnProperty("kill_process")) {
            delete this.processes[message.kill_process];
            this.updateProcesses();
        } else {
            for (let process of Object.values(this.processes))
                process.state = "ready";
            this.runJob(message);
        }
    }

    get onmessage() { return this.onmessage_; }
    set onmessage(f) {
        this.onmessage_ = f;
        setTimeout(this.updateProcesses, 0);
    }
}

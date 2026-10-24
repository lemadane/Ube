// Ube Standard Library - Streaming & Binary I/O Package

class Bytes {
    var int capacity
    var int length

    init(int capacity) {
        this._capacity = capacity
        this._length = 0
    }
}

class InputStream {
    int read(Bytes buffer, int offset, int count) {
        return ube_stream_read(this, buffer, offset, count)
    }

    int pipeTo(OutputStream destination) {
        return ube_stream_pipe_to(this, destination)
    }

    void close() {
        ube_stream_close(this)
    }
}

class OutputStream {
    int write(Bytes buffer, int offset, int count) {
        return ube_stream_write(this, buffer, offset, count)
    }

    void close() {
        ube_stream_close(this)
    }
}

class File {
    static InputStream openRead(str path) {
        return ube_file_open_read(path)
    }

    static OutputStream openWrite(str path) {
        return ube_file_open_write(path)
    }

    static str sha256(str path) {
        return ube_file_sha256(path)
    }
}

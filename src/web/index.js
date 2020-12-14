var failingApiCallHook = null;

const api = (thing) => {
  let opts = typeof thing === 'object' ? {...thing} : {method: 'get', url: thing};
  opts.method = opts.method || (opts.data ? 'post' : 'get');
  if (opts.data) {
    opts.body = JSON.stringify(opts.data);
    opts.headers = opts.headers || {};
    opts.headers['Content-Type'] = 'application/json';
    delete opts['data'];
  }

  const url = opts.url;
  delete opts['url'];
  return fetch(url, opts).then(r => {
    if (!r.ok) {
      if (failingApiCallHook) { failingApiCallHook(r); }
      let err;
      try {
        err = r.json();
      } catch (e) {
        err = e;
      }
      throw new Error(err);
    }
    return r.json();
  });
}

const ErrorToast = ({message, onDismiss}) => {
  return <div style={{position: "absolute", top: "0", "right": 0}}>
           <div className={["toast", message ? "show" : null].join(" ")}>
             <div className="toast-header">
               <strong className="me-auto">Error</strong>
               <small> </small>
               <button type="button" className="btn-close" onClick={(e) => { e.preventDefault(); onDismiss(); }} />
             </div>
             <div className="toast-body">
               {message}
             </div>
           </div>
         </div>;
}

const NavLink = ({id, name, active, setActive}) => {
  return <li className={["nav-item", active === id ? "active" : ""].join(" ")}>
           <a className="nav-link" href="#" onClick={(e) => {e.preventDefault(); setActive(id)}}>{name}</a>
         </li>;
}

const Nav = (props) => {
  const [open, setOpen] = React.useState(false);

  return <nav className="navbar navbar-light bg-light navbar-expand-sm px-4">
    <a className="navbar-brand" href="/">Wink</a>
    <button className="navbar-toggler" type="button" onClick={(e) => { e.preventDefault(); setOpen(v => !v) }}>
      <span className="navbar-toggler-icon"></span>
    </button>
    <div className={[!open ? "collapse" : "", "navbar-collapse"].join(" ")} id="navbarNav">
      <ul className="navbar-nav">
        <NavLink id="home" name="Home" {...props} />
        <NavLink id="add" name="Add Device" {...props} />
        <NavLink id="aprontest" name="aprontest output" {...props} />
      </ul>
    </div>
  </nav>;
}

const Spinner = () => {
  return <div className="d-flex justify-content-center">
           <div className="spinner-border">
           </div>
         </div>;
};

const PencilIcon = () => {
  return <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" fill="currentColor" className="bi bi-pencil-fill" viewBox="0 0 16 16">
           <path fillRule="evenodd" d="M12.854.146a.5.5 0 0 0-.707 0L10.5 1.793 14.207 5.5l1.647-1.646a.5.5 0 0 0 0-.708l-3-3zm.646 6.061L9.793 2.5 3.293 9H3.5a.5.5 0 0 1 .5.5v.5h.5a.5.5 0 0 1 .5.5v.5h.5a.5.5 0 0 1 .5.5v.5h.5a.5.5 0 0 1 .5.5v.207l6.5-6.5zm-7.468 7.468A.5.5 0 0 1 6 13.5V13h-.5a.5.5 0 0 1-.5-.5V12h-.5a.5.5 0 0 1-.5-.5V11h-.5a.5.5 0 0 1-.5-.5V10h-.5a.499.499 0 0 1-.175-.032l-.179.178a.5.5 0 0 0-.11.168l-2 5a.5.5 0 0 0 .65.65l5-2a.5.5 0 0 0 .168-.11l.178-.178z"/>
         </svg>;
}

const findInterestingAttr = (device) => {
  const level = device.attributes.filter(e => e.description === 'Level')[0];
  if (level) {
    return level;
  }

  const on_off = device.attributes.filter(e => e.description === 'On_Off')[0];
  if (on_off) {
    return on_off;
  }

  return null;
}

const DeviceList = ({devicesList, openDeviceDetails}) => {
  return <table className="table">
    <thead>
      <tr>
        <th scope="col">Name</th>
        <th scope="col">Interesting Attribute</th>
        <th scope="col">Details</th>
      </tr>
    </thead>
    <tbody>
      {devicesList.map((v, i) => {
        const interestingAttr = findInterestingAttr(v);
        return <tr key={i}>
          <th scope="row">{v.name}</th>
          <td>{interestingAttr ? interestingAttr.description + ': ' + interestingAttr.current_value : '???'}</td>
          <td><a href="#" onClick={(e) => {e.preventDefault(); openDeviceDetails(v.id)}}>Details</a></td>
        </tr>
      })}
    </tbody>
  </table>;
};

const AttributeControlOnOff = ({attribute, changeValue}) => {
  const isOn = attribute.setting_value === true || ('' + attribute.setting_value).toUpperCase() === 'ON';

  return <div>
    <div className="form-check form-switch">
      <input className="form-check-input" type="checkbox" id="flexSwitchCheckDefault" checked={isOn} onChange={(e) => { e.preventDefault(); changeValue(e.target.checked ? 'ON' : 'OFF')}} />
    </div>
  </div>;
}

const AttributeControlLevel = ({attribute, changeValue}) => {
  const [pendingSet, setPendingSet] = React.useState(null);

  React.useEffect(() => {
    if (!pendingSet) {
      return;
    }

    const timeout = setTimeout(() => {setPendingSet(null)}, 10);
    return () => clearTimeout(timeout);
  }, [pendingSet]);

  let max;
  if (attribute.attribute_type === 'UInt8') {
    max = 255;
  } else if (attribute.attribute_type === 'UInt16') {
    max = 65535;
  } else if (attribute.attribute_type === 'UInt32') {
    max = 4294967295;
  } else {
    return <div>Unknown Level Type: {attribute.attribute_type}</div>
  }

  return <div>
    <input type="range" min="0" max={max} value={attribute.setting_value}
           onChange={(e) => {
             e.preventDefault();
             setPendingSet(e.target.value);
             changeValue(e.target.value)
           }} />
    {pendingSet ? <span>Setting to {pendingSet}...</span> : null}
  </div>
}

const AttributeControl = ({attribute, ...props}) => {
  if (attribute.description === 'On_Off') {
    return <AttributeControlOnOff attribute={attribute} {...props} />;
  } else if (attribute.description === 'Level') {
    return <AttributeControlLevel attribute={attribute} {...props} />;
  } else {
    return <div>Unknown attribute type {attribute.description}</div>;
  }
}

const DeviceDetails = ({device, changeName, setAttribute}) => {
  const [editNameModal, setEditNameModal] = React.useState(false);

  const interestingAttr = findInterestingAttr(device);

  return <div>
  <h1>{device.name} <a href="#" onClick={(e) => {e.preventDefault(); setEditNameModal(true); }} ><PencilIcon /></a></h1>
  <div className="p-3" />
  <div className="d-flex align-items-center">
    <h2>Status</h2>
    <span className="badge bg-secondary mx-3">{device.status}</span>
  </div>
  {interestingAttr ? <>
    <div className="p-3 d-flex flex-column">
    <AttributeControl attribute={interestingAttr} changeValue={(v) => {setAttribute(interestingAttr, v)}} />
    <span><strong>Current Value: </strong> {interestingAttr.current_value + ''}</span></div>
    </>
    :
    <h3>Unknown device type</h3>}
  <div className="p-3" />
  <h2>All Attributes</h2>
  <table className="table">
    <thead>
      <tr>
        <th scope="col">Attribute</th>
        <th scope="col">Features</th>
        <th scope="col">Current Value</th>
        <th scope="col">Setting Value</th>
      </tr>
    </thead>
    <tbody>
      {device.attributes.map((e, i) => {
        return <tr>
          <th scope="row">{e.description}</th>
          <td>{[e.supports_read ? 'R' : null, e.supports_write ? 'W' : null].filter(e => e).join("/")}</td>
          <td>{e.current_value === null ? '' : '' + e.current_value}</td>
          <td>{e.setting_value === null ? '' : '' + e.setting_value}</td>
        </tr>;
      })}
    </tbody>
  </table>
  <div className="p-3" />
  <h2>Full JSON Data</h2>
  <reactJsonView.default name="device" sortKeys={true} src={device} />
  </div>
}

const HomePage = ({device, setDevice}) => {
  const [deviceRefresh, setDeviceRefresh] = React.useState(0);
  const [devicesList, setDevicesList] = React.useState(null);

  React.useEffect(() => {
    api('/api/devices').then(l => setDevicesList(l.devices));
  }, [deviceRefresh]);

  if (!devicesList) {
    return <Spinner />;
  }

  const foundDevice = device && devicesList.filter(e => e.id == device)[0]
  if (foundDevice) {
    return <DeviceDetails device={foundDevice}
                          changeName={(newName) => alert('Not implemented')}
                          setAttribute={(attribute, value) => {
                            api({url: '/api/devices/' + device + '/' + attribute.id, data: {value_text: value}})
                                .then((_) => {setDeviceRefresh(v => v + 1)} )
                          }}
                          />
  } else {
    return <DeviceList devicesList={devicesList} openDeviceDetails={setDevice} />;
  }
}

const AddDevice = () => {
  const [discoveryPending, setDiscoveryPending] = React.useState(false);
  const [discoveryOutput, setDiscoveryOutput] = React.useState(null);

  return <div>
    <form className="d-flex" onSubmit={(e) => {
                                if (discoveryPending) { return; }

                                e.preventDefault();
                                const data = Object.fromEntries(new FormData(e.target));

                                setDiscoveryPending(true);
                                setDiscoveryOutput('');
                                api({url: '/api/devices/discovery', data: data})
                                  .then(v => {
                                    setDiscoveryOutput(
                                      '' + (v.status ? 'OK' : 'ERROR') + '\n\n' +
                                      'Stdout:\n' + v.stdout + '\n\n' +
                                      'Stderr:\n' + v.stderr
                                    );
                                  })
                                  .finally(v => {
                                    setDiscoveryPending(false);
                                  })
                              }}>
      <div className="form-floating flex-grow-1 me-3">
        <select className="form-select" name="radio">
          <option value="zwave">Z-Wave</option>
          <option value="zigbee">Zigbee</option>
          <option value="lutron">Lutron</option>
          <option value="kidde">Kidde</option>
        </select>
        <label>Select Radio Type</label>
      </div>
      <button type="submit" className="btn btn-primary" disabled={discoveryPending}>Start Discovery</button>
    </form>
    <pre className="border d-block mt-3"><code>
      {discoveryPending ? 'Discovery started...' : ''}
      {discoveryOutput}
    </code></pre>
  </div>
};

const RawApronTest = () => {
  const [running, setRunning] = React.useState(false);
  const [output, setOutput] = React.useState(null);

  return <div>
    <form className="d-flex" onSubmit={(e) => {
      e.preventDefault();
      const data = Object.fromEntries(new FormData(e.target));

      setRunning(true);
      setOutput('');
      api({url: '/api/aprontest', data: data})
          .then(v => {
            setOutput(
              '' + (v.status ? 'OK' : 'ERROR') + '\n\n' +
              'Stdout:\n' + v.stdout + '\n\n' +
              'Stderr:\n' + v.stderr
            );
          })
         .finally(() => setRunning(false))
    }}>
      <div className="form-floating flex-grow-1 me-3">
        <input name="command" type="text" className="form-control" defaultValue="aprontest" />
        <label>Type Command</label>
      </div>
      <button type="submit" className="btn btn-primary" disabled={running}>Run</button>
    </form>
    <pre className="border d-block mt-3"><code>
      {running ? 'Running...' : ''}
      {output}
    </code></pre>
  </div>
};

const Root = () => {
  const [active, setActive] = React.useState('home');
  const [device, setDevice] = React.useState(null);
  const [error, setError] = React.useState(null);

  React.useEffect(() => {
    failingApiCallHook = (e) => {
      setError('API Call failed. Error ' + e.status);
    };
    return () => {
      failingApiCallHook = null;
    }
  })

  return <div>
    <Nav active={active} setActive={(active) => { setActive(active); setDevice(null); }} />
    <div className="p-4">
      {active === 'home' ? <HomePage device={device} setDevice={setDevice} /> : null}
      {active === 'add' ? <AddDevice /> : null}
      {active === 'aprontest' ? <RawApronTest /> : null}
    </div>
    <ErrorToast message={error} onDismiss={() => setError(null)} />
  </div>;
};

ReactDOM.render(
  <Root />,
  document.getElementById('app')
);
